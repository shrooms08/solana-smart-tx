//! Jito bundle construction + `sendBundle`.
//!
//! Builds a tip-bearing two-transaction bundle and submits it to a Jito
//! block-engine region over a single pooled HTTP client (getTipAccounts +
//! sendBundle share the connection). The payload tx carries a memo (our tracking
//! marker) plus a wallet-to-wallet self-transfer so the bundle has real
//! account-state-changing content — the auction deprioritizes tip-only no-op
//! bundles, so a self-transfer makes it a legitimate transaction it will
//! schedule. Every submission produces a [`BundleRecord`]
//! capturing exactly *what* was sent (bundle id, tip account + amount, both
//! signatures, the blockhash and the slot it was anchored at, and any injected
//! fault) so the lifecycle/failure layers have explicit, recordable metadata.
//!
//! The slot clock comes from the caller (`current_slot`, sourced from the
//! stream); this crate never calls `getSlot`. On the hot path the blockhash and
//! tip accounts are served from background-refreshed caches, so the only network
//! call during a submission is `sendBundle` itself.

use std::str::FromStr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::Transaction;
use tracing::{debug, info, warn};

/// SPL Memo program id (current `solana-program/memo`). Verified by
/// [`tests::memo_program_id_is_valid`].
pub const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

/// Compute Budget program id. Verified by [`tests::compute_budget_program_id_is_valid`].
pub const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";

/// `ComputeBudgetInstruction::SetComputeUnitPrice` discriminant (the 4th variant,
/// index 3) in the program's borsh-encoded instruction enum.
const SET_COMPUTE_UNIT_PRICE_DISCRIMINANT: u8 = 3;

// ---------------------------------------------------------------------------
// Config / spec
// ---------------------------------------------------------------------------

/// How to pick the tip account for each submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipAccountStrategy {
    /// Use the SDK's random tip account per submission.
    Random,
}

/// Where and how to submit bundles.
#[derive(Debug, Clone)]
pub struct SubmitterConfig {
    /// Jito block-engine base URL, including the API path, e.g.
    /// `https://mainnet.block-engine.jito.wtf/api/v1`.
    pub block_engine_url: String,
    /// Prefix prepended to every memo (e.g. an app tag); may be empty.
    pub memo_prefix: String,
    /// Tip-account selection strategy.
    pub tip_account_strategy: TipAccountStrategy,
    /// Lamports for the wallet -> wallet self-transfer added to the payload tx,
    /// giving the bundle real account-state-changing content so the auction
    /// schedules it (a pure tip-only bundle is a no-op the auction deprioritizes).
    /// Free beyond the per-signature fee. Configured via `SELF_TRANSFER_LAMPORTS`.
    pub self_transfer_lamports: u64,
    /// Requests/second budget for ALL block-engine calls (the shared
    /// [`JitoRateLimiter`]). Jito's anonymous tier is 1; an auth uuid raises it
    /// to 5. Configured via `JITO_RPS`.
    pub jito_rps: u32,
    /// Optional Jito auth uuid. When set, sent as the `x-jito-auth` header on
    /// every block-engine request (raises the rate limit to 5 RPS). `None` =
    /// anonymous (1 RPS). Configured via `JITO_AUTH_UUID`.
    pub auth_uuid: Option<String>,
}

/// A fault to inject into construction (test/chaos only; feature-gated).
#[cfg(feature = "fault-injection")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Anchor on an intentionally older blockhash so the bundle is liable to
    /// fail with blockhash-expired. See [`BundleSubmitter::submit`] for the
    /// staleness actually achievable.
    StaleBlockhash { age_slots: u64 },
    /// Override the tip with a sub-floor value to force a Block Engine rejection.
    SubFloorTip { lamports: u64 },
}

/// One bundle to construct + submit.
#[derive(Debug, Clone)]
pub struct BundleSpec {
    /// Tip amount in lamports (tx2's transfer to the tip account).
    pub tip_lamports: u64,
    /// Memo body (tx1). The final memo is `config.memo_prefix + memo_text`.
    pub memo_text: String,
    /// Priority fee in micro-lamports per compute unit. When `> 0`, a
    /// `ComputeBudget::SetComputeUnitPrice` instruction is added to the payload tx
    /// so the bundle competes in a BAM leader's `(tips + priority_fees) / CU`
    /// auction. `0` (the default) adds nothing — behavior is unchanged. The
    /// orchestrator sets this only for BAM leaders when the feature is enabled.
    pub priority_fee_microlamports: u64,
    /// Optional injected fault (compiled out without `fault-injection`).
    #[cfg(feature = "fault-injection")]
    pub fault: Option<Fault>,
}

// ---------------------------------------------------------------------------
// Record / errors
// ---------------------------------------------------------------------------

/// Recordable metadata for a submitted bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleRecord {
    /// Bundle id returned by `sendBundle`.
    pub bundle_id: String,
    /// Tip amount actually used (after any fault override).
    pub tip_lamports: u64,
    /// Base58 tip account that received the transfer.
    pub tip_account: String,
    /// Base58 signature of the memo tx — what we track on-stream.
    pub memo_signature: String,
    /// Base58 signature of the tip tx.
    pub tip_signature: String,
    /// Base58 blockhash both txs were anchored on.
    pub blockhash: String,
    /// Caller-supplied slot (stream clock) at the time the blockhash was fetched.
    pub blockhash_fetched_at_slot: u64,
    /// When `sendBundle` was issued.
    pub submitted_at: SystemTime,
    /// A description of the injected fault, if any.
    pub fault_injected: Option<String>,
}

/// Typed submission errors. The raw upstream string is always preserved so the
/// failure classifier can pattern-match on it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubmitError {
    /// The block engine accepted the request but rejected the bundle (JSON-RPC
    /// `error`), e.g. sub-floor tip. `reason` is the upstream message.
    #[error("bundle rejected by block engine: {reason}")]
    Rejected { reason: String },
    /// A transport/RPC failure fetching the blockhash/tip account or sending.
    #[error("transport error: {0}")]
    Transport(String),
    /// The response was well-formed HTTP but not parseable as a bundle result.
    #[error("bad response from block engine: {0}")]
    BadResponse(String),
}

// ---------------------------------------------------------------------------
// Background blockhash cache (keeps the blockhash RPC out of the hot path)
// ---------------------------------------------------------------------------

/// A cached blockhash plus the slot it was stamped at, for freshness accounting.
#[derive(Debug, Clone, Copy)]
pub struct CachedBlockhash {
    pub hash: Hash,
    pub fetched_at_slot: u64,
}

/// A shared blockhash cache refreshed in the background. Reading is instant (no
/// RPC), so the submission hot path makes no blockhash round-trip. A blockhash is
/// valid ~150 slots, so a ~2s-old cached value leaves ample validity runway.
#[derive(Clone, Default)]
pub struct BlockhashCache {
    inner: Arc<RwLock<Option<CachedBlockhash>>>,
}

impl BlockhashCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a freshly-fetched blockhash and the slot it was fetched at.
    pub fn store(&self, hash: Hash, fetched_at_slot: u64) {
        *self.inner.write().unwrap() = Some(CachedBlockhash {
            hash,
            fetched_at_slot,
        });
    }

    /// Read the cached blockhash, if one has been stored yet.
    pub fn get(&self) -> Option<CachedBlockhash> {
        *self.inner.read().unwrap()
    }
}

/// Spawn a background task that refreshes `cache` with a fresh **confirmed**
/// blockhash every `interval`, stamping the current slot from `slot_clock`. This
/// removes the ~RPC-round-trip blockhash fetch from the submission hot path.
pub fn spawn_blockhash_refresher(
    cache: BlockhashCache,
    rpc: Arc<solana_client::nonblocking::rpc_client::RpcClient>,
    slot_clock: Arc<AtomicU64>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use solana_commitment_config::CommitmentConfig;
        loop {
            match rpc
                .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
                .await
            {
                Ok((hash, _last_valid)) => {
                    let slot = slot_clock.load(Ordering::Relaxed);
                    cache.store(hash, slot);
                    debug!(blockhash = %hash, at_slot = slot, "blockhash cache refreshed");
                }
                Err(err) => warn!(
                    error = %runtime::redact_url(&err.to_string()),
                    "blockhash refresh failed; keeping last cached value"
                ),
            }
            tokio::time::sleep(interval).await;
        }
    })
}

// ---------------------------------------------------------------------------
// Gateway seam (mockable for offline tests)
// ---------------------------------------------------------------------------

/// The network seam: blockhash + tip-account fetch and `sendBundle`. Abstracted
/// so construction/error-mapping can be unit-tested without a network. The
/// production implementation is [`LiveGateway`] (nonblocking RPC + Jito SDK).
pub trait BundleGateway: Send + Sync {
    /// Fetch a recent blockhash. `finalized` selects finalized vs confirmed
    /// commitment (finalized is used for the stale-blockhash fault).
    fn latest_blockhash(
        &self,
        finalized: bool,
    ) -> impl std::future::Future<Output = anyhow::Result<Hash>> + Send;

    /// A random Jito tip account (base58).
    fn random_tip_account(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send;

    /// Pre-fetch + cache the tip-account list (no return value). Called from a
    /// background task so the hot path reads tip accounts with no network call.
    fn warm_tip_cache(&self) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    /// Submit base64-encoded transactions; returns the raw JSON-RPC response.
    fn send_bundle(
        &self,
        txs_base64: Vec<String>,
    ) -> impl std::future::Future<Output = anyhow::Result<serde_json::Value>> + Send;

    /// Simulate a single base64-encoded transaction (the exact bytes we'd send)
    /// and return its execution result — error, logs, compute units. Used to
    /// catch a tx that would abort and silently drop the whole atomic bundle.
    fn simulate_transaction(
        &self,
        tx_base64: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<SimulationResult>> + Send;

    /// Query Jito for a submitted bundle's status: `getInflightBundleStatuses`
    /// for the live Invalid/Pending/Failed/Landed verdict, enriched (once landed)
    /// with slot + signatures from `getBundleStatuses`. The authoritative signal
    /// for whether an accepted bundle entered the auction and what became of it.
    fn poll_bundle_status(
        &self,
        bundle_id: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<BundleStatusReport>> + Send;
}

/// Result of simulating one transaction.
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// `Some` if the transaction would fail execution (the reason).
    pub err: Option<String>,
    /// Program log lines emitted during simulation.
    pub logs: Vec<String>,
    /// Compute units consumed.
    pub units_consumed: Option<u64>,
}

/// Simulation of one bundle transaction (with its label + signature).
#[derive(Debug, Clone)]
pub struct TxSimulation {
    pub label: &'static str,
    pub signature: String,
    pub result: SimulationResult,
}

/// Per-transaction simulation of a fully-built bundle.
#[derive(Debug, Clone)]
pub struct BundleSimulation {
    pub blockhash: String,
    pub tip_account: String,
    pub tip_lamports: u64,
    pub transactions: Vec<TxSimulation>,
}

/// Jito's authoritative status for a submitted bundle, from
/// `getInflightBundleStatuses`. This is the second signal we compare against our
/// own on-chain reconciliation: it tells us whether an accepted `bundle_id`
/// actually entered the auction and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleStatus {
    /// Not in the Jito system, or the bundle id is more than ~5 minutes old.
    /// (If a freshly-accepted bundle reports `Invalid`, it never entered.)
    Invalid,
    /// In the system but not yet processed (still being auctioned / relayed).
    Pending,
    /// All regions reported it failed, expired, or it landed elsewhere — i.e. it
    /// entered the system but did NOT win/land here.
    Failed,
    /// Landed on-chain.
    Landed,
    /// A status string Jito returned that we don't recognize (forward-compat).
    Unknown(String),
}

impl BundleStatus {
    /// Map Jito's status string (case-insensitive) onto the enum.
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "invalid" => Self::Invalid,
            "pending" => Self::Pending,
            "failed" => Self::Failed,
            "landed" => Self::Landed,
            _ => Self::Unknown(s.to_string()),
        }
    }

    /// Whether this is a terminal state (no further transitions expected), so the
    /// poller can stop early.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Landed | Self::Failed | Self::Invalid)
    }

    /// The status string for logging.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Invalid => "Invalid",
            Self::Pending => "Pending",
            Self::Failed => "Failed",
            Self::Landed => "Landed",
            Self::Unknown(s) => s,
        }
    }
}

/// Combined status report for one bundle: the inflight status, enriched (once
/// landed) with the slot, confirmation status, and tx signatures from
/// `getBundleStatuses`.
#[derive(Debug, Clone)]
pub struct BundleStatusReport {
    pub bundle_id: String,
    pub status: BundleStatus,
    /// Slot the bundle landed in (from `getBundleStatuses`), once available.
    pub landed_slot: Option<u64>,
    /// Confirmation status from `getBundleStatuses` (processed/confirmed/finalized).
    pub confirmation_status: Option<String>,
    /// Bundle transaction signatures (from `getBundleStatuses`), once available.
    pub signatures: Vec<String>,
}

/// How long a fetched tip-account list stays valid before a refetch.
const TIP_ACCOUNTS_TTL: Duration = Duration::from_secs(300);

/// Upper bound on how long we'll honor Jito's `x-wait-to-retry-ms` 429 hint
/// before failing the send (so the retry loop, not this call, owns longer waits).
const WAIT_TO_RETRY_CAP_MS: u64 = 2_000;

/// Cached tip-account list with its fetch time.
struct TipAccountsCache {
    accounts: Vec<String>,
    fetched_at: Instant,
}

/// Call priority for the shared Jito rate limiter. `High` (sendBundle) reserves
/// its token immediately; `Low` (background warmer + status poller) yields to any
/// pending `High` acquisition so a submission never starves behind background
/// traffic on the 1-RPS anonymous budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    High,
    Low,
}

/// A shared async token-bucket limiter for ALL block-engine calls (getTipAccounts,
/// sendBundle, getInflightBundleStatuses, getBundleStatuses). Jito throttles the
/// anonymous public endpoint to 1 request/sec/IP/region (5 with an auth uuid);
/// exceeding it returns HTTP 429. We pace ourselves to that budget so our own
/// background traffic can't 429 a live submission.
///
/// Prioritization: a slot is "reserved" by advancing `next_at` under a mutex. A
/// `High` caller bumps `high_pending` first, so concurrent `Low` callers spin-wait
/// (one interval at a time) until no `High` is pending — i.e. sendBundle always
/// jumps ahead of the warmer/poller for the next available token.
pub struct JitoRateLimiter {
    /// Minimum spacing between requests (1 / rps).
    interval: Duration,
    /// Earliest instant the next request may go. Reserved by advancing it.
    next_at: tokio::sync::Mutex<Instant>,
    /// Count of in-flight `High` acquisitions; `Low` yields while this is > 0.
    high_pending: AtomicUsize,
}

impl JitoRateLimiter {
    /// Build a limiter for `rps` requests/second (clamped to >= 1).
    pub fn new(rps: u32) -> Self {
        let rps = rps.max(1);
        Self {
            interval: Duration::from_secs(1) / rps,
            next_at: tokio::sync::Mutex::new(Instant::now()),
            high_pending: AtomicUsize::new(0),
        }
    }

    /// Acquire one token, sleeping until the bucket allows it. `High` reserves the
    /// next slot ahead of any waiting `Low` caller.
    pub async fn acquire(&self, priority: Priority) {
        if priority == Priority::High {
            self.high_pending.fetch_add(1, Ordering::SeqCst);
        } else {
            // Yield the upcoming slot to any pending high-priority sendBundle.
            while self.high_pending.load(Ordering::SeqCst) > 0 {
                tokio::time::sleep(self.interval).await;
            }
        }

        let wait = {
            let mut next = self.next_at.lock().await;
            let now = Instant::now();
            let start = (*next).max(now);
            *next = start + self.interval;
            start.saturating_duration_since(now)
        };

        if priority == Priority::High {
            // Slot is reserved (next_at advanced); release the priority hold so a
            // waiting Low caller can take the slot *after* ours.
            self.high_pending.fetch_sub(1, Ordering::SeqCst);
        }
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

/// Production gateway: nonblocking Solana RPC for the blockhash, a direct HTTP
/// `getTipAccounts` fetch (the SDK helper returns an empty body for us), and the
/// Jito SDK for `sendBundle` (the one path that isn't implicated).
pub struct LiveGateway {
    rpc: solana_client::nonblocking::rpc_client::RpcClient,
    /// ONE pooled HTTP client shared by getTipAccounts AND sendBundle (same
    /// block-engine host), so a connection warmed by one is reused by the other —
    /// the TLS handshake is paid once, not per call.
    http: reqwest::Client,
    /// `{block_engine_url}/api/v1/getTipAccounts`.
    tip_accounts_url: String,
    /// `{block_engine_url}/api/v1/bundles` (the `sendBundle` JSON-RPC endpoint).
    bundles_url: String,
    /// `{block_engine_url}/api/v1/getInflightBundleStatuses`.
    inflight_statuses_url: String,
    /// `{block_engine_url}/api/v1/getBundleStatuses`.
    bundle_statuses_url: String,
    tip_cache: Mutex<Option<TipAccountsCache>>,
    /// Shared limiter every block-engine call passes through (see [`JitoRateLimiter`]).
    limiter: Arc<JitoRateLimiter>,
    /// Optional Jito auth uuid -> `x-jito-auth` header on every request.
    auth_uuid: Option<String>,
}

impl LiveGateway {
    fn new(
        rpc: solana_client::nonblocking::rpc_client::RpcClient,
        block_engine_url: &str,
        jito_rps: u32,
        auth_uuid: Option<String>,
    ) -> Self {
        // The configured base is the bare host (no `/api/v1`); both endpoints live
        // under `{host}/api/v1/...`.
        let api_base = format!("{}/api/v1", block_engine_url.trim_end_matches('/'));
        // Persistent connection pool / keep-alive so we don't re-handshake TLS on
        // every submission. HTTP/2 is negotiated automatically via ALPN.
        let http = reqwest::Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(4)
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            rpc,
            http,
            tip_accounts_url: format!("{api_base}/getTipAccounts"),
            bundles_url: format!("{api_base}/bundles"),
            inflight_statuses_url: format!("{api_base}/getInflightBundleStatuses"),
            bundle_statuses_url: format!("{api_base}/getBundleStatuses"),
            tip_cache: Mutex::new(None),
            limiter: Arc::new(JitoRateLimiter::new(jito_rps)),
            auth_uuid,
        }
    }

    /// Acquire a rate-limit token (at `priority`), then POST `body` to a
    /// block-engine `url` over the pooled client with the `x-jito-auth` header (if
    /// configured). The single choke point all Jito traffic flows through.
    async fn jito_send(
        &self,
        url: &str,
        body: &serde_json::Value,
        priority: Priority,
    ) -> anyhow::Result<reqwest::Response> {
        self.limiter.acquire(priority).await;
        let mut req = self.http.post(url).json(body);
        if let Some(uuid) = &self.auth_uuid {
            req = req.header("x-jito-auth", uuid);
        }
        req.send()
            .await
            .map_err(|e| anyhow::anyhow!("error sending request: {e}"))
    }

    /// POST a JSON-RPC body to a block-engine endpoint (rate-limited + authed) and
    /// return the response text (erroring on a non-2xx status).
    async fn post_text(
        &self,
        url: &str,
        body: &serde_json::Value,
        priority: Priority,
    ) -> anyhow::Result<String> {
        let response = self.jito_send(url, body, priority).await?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("error decoding response body: {e}"))?;
        if !status.is_success() {
            anyhow::bail!("HTTP {}: {text}", status.as_u16());
        }
        Ok(text)
    }

    /// One `getInflightBundleStatuses` call for a single bundle id. Background
    /// poll -> `Low` priority (yields to a live sendBundle).
    async fn fetch_inflight_status(&self, bundle_id: &str) -> anyhow::Result<BundleStatus> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getInflightBundleStatuses",
            "params": [[bundle_id]],
        });
        let text = self
            .post_text(&self.inflight_statuses_url, &body, Priority::Low)
            .await?;
        parse_inflight_status(&text, bundle_id)
    }

    /// One `getBundleStatuses` call for a single bundle id, returning
    /// (landed_slot, confirmation_status, signatures) when present. `Low` priority.
    async fn fetch_bundle_detail(
        &self,
        bundle_id: &str,
    ) -> anyhow::Result<(Option<u64>, Option<String>, Vec<String>)> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBundleStatuses",
            "params": [[bundle_id]],
        });
        let text = self
            .post_text(&self.bundle_statuses_url, &body, Priority::Low)
            .await?;
        parse_bundle_detail(&text, bundle_id)
    }

    /// Fetch the tip-account list over HTTP and store it in the cache, returning
    /// the accounts. Also warms the keep-alive connection that `send_bundle`
    /// reuses (same host, same `http` client).
    async fn refresh_tip_cache(&self) -> anyhow::Result<Vec<String>> {
        let accounts = self.fetch_tip_accounts().await?;
        *self.tip_cache.lock().unwrap() = Some(TipAccountsCache {
            accounts: accounts.clone(),
            fetched_at: Instant::now(),
        });
        Ok(accounts)
    }

    /// Return a cached tip account if the cache is fresh and non-empty.
    fn cached_tip_account(&self) -> Option<String> {
        let guard = self.tip_cache.lock().unwrap();
        let cache = guard.as_ref()?;
        if cache.fetched_at.elapsed() < TIP_ACCOUNTS_TTL && !cache.accounts.is_empty() {
            choose_account(&cache.accounts)
        } else {
            None
        }
    }

    /// Fetch the tip-account list directly over HTTP (JSON-RPC `getTipAccounts`).
    async fn fetch_tip_accounts(&self) -> anyhow::Result<Vec<String>> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTipAccounts",
            "params": []
        });
        // Background warmer -> `Low` priority (yields to a live sendBundle).
        let response = self
            .jito_send(&self.tip_accounts_url, &body, Priority::Low)
            .await?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("error decoding response body: {e}"))?;
        if !status.is_success() {
            anyhow::bail!("getTipAccounts HTTP {}: {text}", status.as_u16());
        }
        parse_tip_accounts(&text)
    }
}

/// Parse a `getTipAccounts` JSON-RPC response body into the list of base58
/// accounts. An empty body surfaces a serde "EOF while parsing" error (the exact
/// transport failure we hit on mainnet), which the classifier maps to
/// `TransportError` rather than an auction rejection.
fn parse_tip_accounts(body: &str) -> anyhow::Result<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("error decoding response body: {e}"))?;
    let result = value
        .get("result")
        .and_then(|r| r.as_array())
        .ok_or_else(|| anyhow::anyhow!("getTipAccounts: missing 'result' array"))?;
    let accounts: Vec<String> = result
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    if accounts.is_empty() {
        anyhow::bail!("getTipAccounts: empty result array");
    }
    Ok(accounts)
}

/// Parse a `getInflightBundleStatuses` response into our [`BundleStatus`] for
/// `bundle_id`. The response shape is `result.value: [{bundle_id, status, ...}]`;
/// an absent / null entry for our id means Jito doesn't know it -> `Invalid`.
fn parse_inflight_status(body: &str, bundle_id: &str) -> anyhow::Result<BundleStatus> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("error decoding response body: {e}"))?;
    if let Some(err) = value.get("error") {
        anyhow::bail!("getInflightBundleStatuses error: {err}");
    }
    let entries = value
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_array());
    let Some(entries) = entries else {
        // No `value` array at all — treat as not-in-system.
        return Ok(BundleStatus::Invalid);
    };
    for entry in entries {
        if entry.get("bundle_id").and_then(|b| b.as_str()) == Some(bundle_id) {
            let status = entry
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("Invalid");
            return Ok(BundleStatus::parse(status));
        }
    }
    // Our id wasn't in the returned set -> Jito has no record of it.
    Ok(BundleStatus::Invalid)
}

/// Parse a `getBundleStatuses` response into (landed_slot, confirmation_status,
/// signatures) for `bundle_id`. A null / missing entry yields all-empty.
fn parse_bundle_detail(
    body: &str,
    bundle_id: &str,
) -> anyhow::Result<(Option<u64>, Option<String>, Vec<String>)> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("error decoding response body: {e}"))?;
    if let Some(err) = value.get("error") {
        anyhow::bail!("getBundleStatuses error: {err}");
    }
    let entries = value
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_array());
    let Some(entries) = entries else {
        return Ok((None, None, Vec::new()));
    };
    for entry in entries {
        if entry.get("bundle_id").and_then(|b| b.as_str()) == Some(bundle_id) {
            let slot = entry.get("slot").and_then(|s| s.as_u64());
            let conf = entry
                .get("confirmation_status")
                .and_then(|c| c.as_str())
                .map(str::to_owned);
            let sigs = entry
                .get("transactions")
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            return Ok((slot, conf, sigs));
        }
    }
    Ok((None, None, Vec::new()))
}

/// Pick one account from a non-empty list. Selection is time-seeded
/// (`now_nanos % len`) — no `rand` dependency; it just spreads submissions
/// across the ~8 tip accounts. Returns `None` only for an empty slice.
fn choose_account(accounts: &[String]) -> Option<String> {
    if accounts.is_empty() {
        return None;
    }
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    accounts.get(seed % accounts.len()).cloned()
}

impl BundleGateway for LiveGateway {
    async fn latest_blockhash(&self, finalized: bool) -> anyhow::Result<Hash> {
        use solana_commitment_config::CommitmentConfig;
        let commitment = if finalized {
            CommitmentConfig::finalized()
        } else {
            CommitmentConfig::confirmed()
        };
        let (hash, _last_valid) = self
            .rpc
            .get_latest_blockhash_with_commitment(commitment)
            .await?;
        Ok(hash)
    }

    async fn random_tip_account(&self) -> anyhow::Result<String> {
        // Fast path: read from the (background-warmed) cache, NO network call.
        if let Some(account) = self.cached_tip_account() {
            return Ok(account);
        }
        // Cold (cache empty/expired) — fetch + cache, then choose.
        let accounts = self.refresh_tip_cache().await?;
        choose_account(&accounts).ok_or_else(|| anyhow::anyhow!("getTipAccounts returned no accounts"))
    }

    async fn warm_tip_cache(&self) -> anyhow::Result<()> {
        self.refresh_tip_cache().await.map(|_| ())
    }

    async fn send_bundle(&self, txs_base64: Vec<String>) -> anyhow::Result<serde_json::Value> {
        // sendBundle over our OWN pooled client at HIGH priority — it acquires the
        // rate-limit token ahead of background warmer/poller traffic. Params are
        // the fully-formed `[[tx1, tx2], {encoding: base64}]`.
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendBundle",
            "params": [txs_base64, { "encoding": "base64" }],
        });
        let response = self
            .jito_send(&self.bundles_url, &body, Priority::High)
            .await?;
        let status = response.status();
        // Capture headers BEFORE consuming the body. Jito may signal rate-limit /
        // error state in `x-*` headers, and an ingress warning/error alongside the
        // uuid in the body — log ALL of it verbatim so the full response is visible.
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<non-utf8>").to_string()))
            .collect();
        // Jito returns `x-wait-to-retry-ms` on a 429 — honor it when nonzero.
        let wait_to_retry_ms = response
            .headers()
            .get("x-wait-to-retry-ms")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        let text = response
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("error decoding response body: {e}"))?;
        info!(
            http_status = status.as_u16(),
            headers = ?headers,
            body = %text,
            "===== RAW sendBundle RESPONSE (verbatim) ====="
        );
        if status.as_u16() == 429 {
            // Rate limited: respect the server's backoff hint (capped), then bail
            // with a string the failure classifier maps to TransportError so the
            // retry path holds-and-resubmits without touching tip/blockhash.
            if let Some(ms) = wait_to_retry_ms.filter(|ms| *ms > 0) {
                let capped = ms.min(WAIT_TO_RETRY_CAP_MS);
                warn!(
                    wait_to_retry_ms = ms,
                    slept_ms = capped,
                    "sendBundle 429: honoring x-wait-to-retry-ms before failing"
                );
                tokio::time::sleep(Duration::from_millis(capped)).await;
            }
            anyhow::bail!("sendBundle HTTP 429 (globally rate limited): {text}");
        }
        if !status.is_success() {
            anyhow::bail!("sendBundle HTTP {}: {text}", status.as_u16());
        }
        serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("error decoding response body: {e}"))
    }

    async fn simulate_transaction(&self, tx_base64: &str) -> anyhow::Result<SimulationResult> {
        use solana_client::rpc_request::RpcRequest;
        use solana_client::rpc_response::{Response, RpcSimulateTransactionResult};

        // Call `simulateTransaction` directly with the exact base64 we'd send.
        // (We can't use the typed `simulate_transaction_with_config` because the
        // rpc-client's `SerializableTransaction` is bound to solana-transaction
        // 3.1.0 while solana-sdk gives us 4.0.0 — a version skew. The raw request
        // sidesteps it.)
        //
        // FAITHFUL mode: `sigVerify: true` + `replaceRecentBlockhash: false` tests
        // our EXACT signed bytes against OUR real blockhash — so a stale blockhash
        // surfaces as BlockhashNotFound and a bad signature as a sig-verify error,
        // instead of being masked. This is what a validator/Jito actually sees.
        let config = serde_json::json!({
            "sigVerify": true,
            "replaceRecentBlockhash": false,
            "commitment": "confirmed",
            "encoding": "base64",
        });
        let response: Response<RpcSimulateTransactionResult> = self
            .rpc
            .send(
                RpcRequest::SimulateTransaction,
                serde_json::json!([tx_base64, config]),
            )
            .await?;
        let value = response.value;
        Ok(SimulationResult {
            err: value.err.map(|e| format!("{e:?}")),
            logs: value.logs.unwrap_or_default(),
            units_consumed: value.units_consumed,
        })
    }

    async fn poll_bundle_status(&self, bundle_id: &str) -> anyhow::Result<BundleStatusReport> {
        // Cheap, fast verdict first: Invalid / Pending / Failed / Landed.
        let status = self.fetch_inflight_status(bundle_id).await?;
        // Once landed, enrich with the slot + signatures (best-effort: a detail
        // failure shouldn't sink the primary status signal).
        let (landed_slot, confirmation_status, signatures) = if status == BundleStatus::Landed {
            self.fetch_bundle_detail(bundle_id)
                .await
                .unwrap_or((None, None, Vec::new()))
        } else {
            (None, None, Vec::new())
        };
        Ok(BundleStatusReport {
            bundle_id: bundle_id.to_string(),
            status,
            landed_slot,
            confirmation_status,
            signatures,
        })
    }
}

// ---------------------------------------------------------------------------
// Construction (pure; unit-tested)
// ---------------------------------------------------------------------------

fn memo_program_id() -> Pubkey {
    Pubkey::from_str(MEMO_PROGRAM_ID).expect("hardcoded memo program id is valid")
}

/// SPL Memo instruction: program = memo v2, no account metas, data = UTF-8 memo.
fn memo_instruction(memo: &str) -> Instruction {
    Instruction {
        program_id: memo_program_id(),
        accounts: Vec::new(),
        data: memo.as_bytes().to_vec(),
    }
}

fn compute_budget_program_id() -> Pubkey {
    Pubkey::from_str(COMPUTE_BUDGET_PROGRAM_ID).expect("hardcoded compute budget id is valid")
}

/// `ComputeBudget::SetComputeUnitPrice(micro_lamports)` — sets a priority fee of
/// `micro_lamports` per compute unit. No account metas; data is the borsh-encoded
/// enum: a 1-byte discriminant (3) followed by the u64 price (little-endian).
/// Hand-built (like [`memo_instruction`]) to avoid a compute-budget crate dep;
/// the encoding is pinned by [`tests::compute_unit_price_instruction_encoding`].
fn compute_unit_price_instruction(micro_lamports: u64) -> Instruction {
    let mut data = Vec::with_capacity(9);
    data.push(SET_COMPUTE_UNIT_PRICE_DISCRIMINANT);
    data.extend_from_slice(&micro_lamports.to_le_bytes());
    Instruction {
        program_id: compute_budget_program_id(),
        accounts: Vec::new(),
        data,
    }
}

/// The constructed, signed pair plus their wire encodings.
struct BuiltBundle {
    // Retained for offline test inspection; `submit` only sends the encodings.
    #[allow(dead_code)]
    memo_tx: Transaction,
    #[allow(dead_code)]
    tip_tx: Transaction,
    memo_signature: String,
    tip_signature: String,
    memo_base64: String,
    tip_base64: String,
}

/// Build the two-transaction bundle:
///   tx1 = [memo] + [self-transfer: wallet -> wallet, `self_transfer_lamports`]
///   tx2 = [tip transfer: wallet -> Jito tip account]
///
/// The self-transfer gives tx1 real account-state-changing content so the
/// auction treats the bundle as a legitimate transaction rather than a tip-only
/// no-op (see the module docs). It is free beyond the per-signature fee (the
/// wallet pays itself). The memo is retained as our on-chain tracking marker.
/// Both txs are signed by `keypair` (the fee payer) and share `blockhash`.
///
/// When `priority_fee_microlamports > 0`, a `SetComputeUnitPrice` instruction is
/// prepended to tx1 so the bundle pays a priority fee — competing in a BAM
/// leader's `(tips + priority_fees) / CU` auction. `0` adds nothing (unchanged).
fn build_bundle(
    keypair: &Keypair,
    memo: &str,
    self_transfer_lamports: u64,
    priority_fee_microlamports: u64,
    tip_lamports: u64,
    tip_account: &Pubkey,
    blockhash: Hash,
) -> anyhow::Result<BuiltBundle> {
    let payer = keypair.pubkey();

    // tx1: [optional priority fee] + memo (tracking marker) + self-transfer
    // (real economic content). The priority fee goes first, per convention.
    let self_transfer_ix =
        solana_system_interface::instruction::transfer(&payer, &payer, self_transfer_lamports);
    let mut payload_ixs = Vec::with_capacity(3);
    if priority_fee_microlamports > 0 {
        payload_ixs.push(compute_unit_price_instruction(priority_fee_microlamports));
    }
    payload_ixs.push(memo_instruction(memo));
    payload_ixs.push(self_transfer_ix);
    let memo_tx =
        Transaction::new_signed_with_payer(&payload_ixs, Some(&payer), &[keypair], blockhash);

    let transfer_ix =
        solana_system_interface::instruction::transfer(&payer, tip_account, tip_lamports);
    let tip_tx =
        Transaction::new_signed_with_payer(&[transfer_ix], Some(&payer), &[keypair], blockhash);

    Ok(BuiltBundle {
        memo_signature: memo_tx.signatures[0].to_string(),
        tip_signature: tip_tx.signatures[0].to_string(),
        memo_base64: encode_tx(&memo_tx)?,
        tip_base64: encode_tx(&tip_tx)?,
        memo_tx,
        tip_tx,
    })
}

/// bincode-serialize then base64-encode, as Jito's `sendBundle` expects.
fn encode_tx(tx: &Transaction) -> anyhow::Result<String> {
    Ok(BASE64.encode(bincode::serialize(tx)?))
}

// ---------------------------------------------------------------------------
// Bundle diagnostic: decode the fully-built bundle and inspect the tip transfer
// ---------------------------------------------------------------------------

/// System program id — the program a real SOL transfer (the Jito tip) targets.
const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
/// Jito's documented minimum tip (lamports). Below this, Jito never schedules.
pub const JITO_MIN_TIP_LAMPORTS: u64 = 1_000;

/// A decoded `SystemProgram::transfer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedTransfer {
    pub from: Pubkey,
    pub to: Pubkey,
    pub lamports: u64,
}

/// A decoded instruction, resolved against its message's account keys.
#[derive(Debug, Clone)]
pub struct DecodedInstruction {
    pub program_id: Pubkey,
    pub accounts: Vec<Pubkey>,
    pub data_len: usize,
    /// `Some` iff this is a `SystemProgram::transfer`.
    pub transfer: Option<DecodedTransfer>,
}

/// A decoded bundle transaction.
#[derive(Debug, Clone)]
pub struct DecodedBundleTx {
    pub fee_payer: Pubkey,
    pub recent_blockhash: Hash,
    pub num_signatures: usize,
    pub account_keys: Vec<Pubkey>,
    pub instructions: Vec<DecodedInstruction>,
}

fn system_program_id() -> Pubkey {
    Pubkey::from_str(SYSTEM_PROGRAM_ID).expect("valid system program id")
}

/// Decode the base64 + bincode bytes we actually send to Jito back into an
/// inspectable structure — proving the wire bytes round-trip and exposing the
/// real program ids / accounts / transfer amount. Never panics.
pub fn decode_bundle_tx(tx_base64: &str) -> anyhow::Result<DecodedBundleTx> {
    let bytes = BASE64
        .decode(tx_base64)
        .map_err(|e| anyhow::anyhow!("base64 decode failed: {e}"))?;
    let tx: Transaction =
        bincode::deserialize(&bytes).map_err(|e| anyhow::anyhow!("bincode decode failed: {e}"))?;

    let system_id = system_program_id();
    let keys = &tx.message.account_keys;
    let resolve = |idx: u8| keys.get(idx as usize).copied().unwrap_or_default();

    let instructions = tx
        .message
        .instructions
        .iter()
        .map(|ci| {
            let program_id = resolve(ci.program_id_index);
            let accounts: Vec<Pubkey> = ci.accounts.iter().map(|a| resolve(*a)).collect();
            let transfer = decode_transfer(program_id, system_id, &accounts, &ci.data);
            DecodedInstruction {
                program_id,
                accounts,
                data_len: ci.data.len(),
                transfer,
            }
        })
        .collect();

    Ok(DecodedBundleTx {
        fee_payer: keys.first().copied().unwrap_or_default(),
        recent_blockhash: tx.message.recent_blockhash,
        num_signatures: tx.signatures.len(),
        account_keys: keys.clone(),
        instructions,
    })
}

/// Decode a `SystemProgram::transfer` if `data` matches its layout: enum index 2
/// (u32 LE) + lamports (u64 LE), program = system, with `[from, to]` accounts.
fn decode_transfer(
    program_id: Pubkey,
    system_id: Pubkey,
    accounts: &[Pubkey],
    data: &[u8],
) -> Option<DecodedTransfer> {
    if program_id == system_id && data.len() == 12 && data[0..4] == [2, 0, 0, 0] && accounts.len() >= 2
    {
        let lamports = u64::from_le_bytes(data[4..12].try_into().ok()?);
        Some(DecodedTransfer {
            from: accounts[0],
            to: accounts[1],
            lamports,
        })
    } else {
        None
    }
}

/// Whether the bundle contains a valid Jito tip: a `SystemProgram::transfer` of
/// exactly `tip_lamports` (>= the Jito minimum) from `payer` to `tip_account`.
/// Pure; used by both the diagnostic and the tests.
pub fn bundle_has_valid_tip(
    txs_base64: &[String],
    tip_account: &Pubkey,
    tip_lamports: u64,
    payer: &Pubkey,
) -> bool {
    txs_base64.iter().any(|b64| {
        decode_bundle_tx(b64).is_ok_and(|d| {
            d.instructions.iter().any(|ix| {
                ix.transfer.is_some_and(|t| {
                    t.from == *payer
                        && t.to == *tip_account
                        && t.lamports == tip_lamports
                        && t.lamports >= JITO_MIN_TIP_LAMPORTS
                })
            })
        })
    })
}

/// Decode the fully-built bundle and log its complete structure at `debug` level
/// (enable with `RUST_LOG=submitter=debug`), plus self-audit the critical
/// tip-transfer invariants — warning loudly on any mismatch. Diagnostic only;
/// it does not change what is sent.
fn log_bundle_diagnostic(
    txs_base64: &[String],
    expected_tip_account: &Pubkey,
    expected_tip_lamports: u64,
    expected_payer: &Pubkey,
) {
    debug!(
        bundle_tx_count = txs_base64.len(),
        expected_payer = %expected_payer,
        expected_tip_account = %expected_tip_account,
        expected_tip_lamports,
        jito_min_tip = JITO_MIN_TIP_LAMPORTS,
        "BUNDLE DIAGNOSTIC: decoding fully-built bundle (tip transfer should be the LAST tx)"
    );

    for (idx, b64) in txs_base64.iter().enumerate() {
        let is_last = idx + 1 == txs_base64.len();
        match decode_bundle_tx(b64) {
            Err(err) => warn!(
                tx_index = idx,
                error = %err,
                "BUNDLE DIAGNOSTIC: could not decode tx (possible encoding bug)"
            ),
            Ok(d) => {
                debug!(
                    tx_index = idx,
                    is_last,
                    fee_payer = %d.fee_payer,
                    recent_blockhash = %d.recent_blockhash,
                    num_signatures = d.num_signatures,
                    num_accounts = d.account_keys.len(),
                    num_instructions = d.instructions.len(),
                    "BUNDLE DIAGNOSTIC: transaction"
                );
                for (i, ix) in d.instructions.iter().enumerate() {
                    let accounts: Vec<String> =
                        ix.accounts.iter().map(|p| p.to_string()).collect();
                    debug!(
                        tx_index = idx,
                        ix_index = i,
                        program_id = %ix.program_id,
                        accounts = ?accounts,
                        data_len = ix.data_len,
                        "BUNDLE DIAGNOSTIC: instruction"
                    );
                    if let Some(t) = ix.transfer {
                        debug!(
                            tx_index = idx,
                            ix_index = i,
                            transfer_from = %t.from,
                            transfer_to = %t.to,
                            lamports = t.lamports,
                            "BUNDLE DIAGNOSTIC: decoded SystemProgram::transfer"
                        );
                    }
                }
            }
        }
    }

    if bundle_has_valid_tip(
        txs_base64,
        expected_tip_account,
        expected_tip_lamports,
        expected_payer,
    ) {
        debug!(
            "BUNDLE DIAGNOSTIC: OK — a valid tip transfer (wallet -> fetched tip account, \
             exact amount, >= Jito minimum) is present in the bundle"
        );
    } else {
        warn!(
            expected_payer = %expected_payer,
            expected_tip_account = %expected_tip_account,
            expected_tip_lamports,
            "BUNDLE DIAGNOSTIC: NO valid tip transfer found — Jito will accept the submission \
             (return a bundle_id) but never schedule it. Check: is the transfer present, to the \
             fetched tip account, from the wallet, of the exact amount, and >= 1000 lamports?"
        );
    }
}

/// Parse the `sendBundle` JSON-RPC response into a bundle id, mapping a
/// JSON-RPC `error` to [`SubmitError::Rejected`].
fn parse_send_response(response: &serde_json::Value) -> Result<String, SubmitError> {
    if let Some(error) = response.get("error") {
        let reason = error
            .get("message")
            .and_then(|m| m.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string());
        return Err(SubmitError::Rejected { reason });
    }
    match response.get("result").and_then(|r| r.as_str()) {
        Some(id) => Ok(id.to_string()),
        None => Err(SubmitError::BadResponse(response.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Submitter
// ---------------------------------------------------------------------------

/// Constructs and submits Jito bundles.
pub struct BundleSubmitter<G = LiveGateway> {
    config: SubmitterConfig,
    gateway: G,
    keypair: Keypair,
    /// Background-refreshed blockhash; when set, the hot path reads it instead of
    /// an RPC call (the stale-blockhash fault still bypasses it).
    blockhash_cache: Option<BlockhashCache>,
}

impl BundleSubmitter<LiveGateway> {
    /// Construct against live infrastructure: a nonblocking Solana RPC client
    /// (blockhash source) and the signing keypair. The Jito SDK is built
    /// internally from `config.block_engine_url`.
    pub fn new(
        config: SubmitterConfig,
        rpc_client: solana_client::nonblocking::rpc_client::RpcClient,
        keypair: Keypair,
    ) -> Self {
        let gateway = LiveGateway::new(
            rpc_client,
            &config.block_engine_url,
            config.jito_rps,
            config.auth_uuid.clone(),
        );
        Self {
            config,
            gateway,
            keypair,
            blockhash_cache: None,
        }
    }
}

impl<G: BundleGateway> BundleSubmitter<G> {
    /// Construct over an arbitrary gateway (used by tests).
    pub fn with_gateway(config: SubmitterConfig, gateway: G, keypair: Keypair) -> Self {
        Self {
            config,
            gateway,
            keypair,
            blockhash_cache: None,
        }
    }

    /// Attach a background-refreshed blockhash cache. With it set, the hot path
    /// reads the cached blockhash (no RPC round-trip); the stale-blockhash fault
    /// still bypasses the cache and fetches a finalized (old) blockhash.
    pub fn with_blockhash_cache(mut self, cache: BlockhashCache) -> Self {
        self.blockhash_cache = Some(cache);
        self
    }

    /// Pre-fetch + cache the Jito tip accounts (call once at startup, then
    /// periodically in the background). Keeps the hot path free of the
    /// ~getTipAccounts round-trip and warms the keep-alive connection that
    /// `send_bundle` reuses.
    pub async fn warm_tip_cache(&self) -> anyhow::Result<()> {
        self.gateway.warm_tip_cache().await
    }

    /// Query Jito for an accepted bundle's status (see
    /// [`BundleGateway::poll_bundle_status`]). The authoritative, off-chain signal
    /// for whether the bundle entered the auction and what became of it; the
    /// orchestrator polls this alongside its own on-chain reconciliation.
    pub async fn bundle_status(&self, bundle_id: &str) -> anyhow::Result<BundleStatusReport> {
        self.gateway.poll_bundle_status(bundle_id).await
    }

    fn full_memo(&self, text: &str) -> String {
        format!("{}{}", self.config.memo_prefix, text)
    }

    /// Construct and submit the bundle, returning its [`BundleRecord`].
    ///
    /// `current_slot` (the stream clock) is recorded as
    /// `blockhash_fetched_at_slot`; this method does not call `getSlot`.
    ///
    /// ## Stale-blockhash fault, in practice
    ///
    /// `Fault::StaleBlockhash { age_slots }` cannot honor `age_slots` precisely:
    /// the RPC only ever hands back a *current* blockhash for a given
    /// commitment. The best we can do offline is fetch at **finalized**
    /// commitment, which yields a hash ~31 slots behind `processed`. That is
    /// still inside the ~150-slot expiry window, so on its own it will not
    /// reliably expire — true blockhash-expiry needs the bundle to sit until the
    /// hash ages out. What this fault *reliably* does: anchor on the oldest hash
    /// the RPC will give us and mark the record (`fault_injected`).
    pub async fn submit(
        &self,
        spec: BundleSpec,
        current_slot: u64,
    ) -> Result<BundleRecord, SubmitError> {
        // 1-4. Resolve faults, blockhash (cached) + tip account, build + sign.
        let t_prepare = Instant::now();
        let prep = self.prepare(&spec, current_slot).await?;
        let prepare_ms = t_prepare.elapsed().as_millis();

        debug!(
            tip_lamports = prep.tip_lamports,
            tip_account = %prep.tip_account_str,
            blockhash = %prep.blockhash,
            memo_signature = %prep.built.memo_signature,
            fault = ?prep.fault_injected,
            prepare_ms,
            "constructed bundle (prepare = fetch blockhash + fetch tip account + build + sign)"
        );

        // 5. Diagnostic: decode the exact bytes we're about to send and verify the
        // tip transfer is structurally a valid Jito tip. The tip tx is LAST.
        let txs_base64 = vec![prep.built.memo_base64, prep.built.tip_base64];
        log_bundle_diagnostic(
            &txs_base64,
            &prep.tip_account,
            prep.tip_lamports,
            &self.keypair.pubkey(),
        );

        // 5b. Debug-gated pre-submission simulation (SIMULATE_BEFORE_SEND=1): a
        // tx that aborts in simulation would silently drop the whole atomic bundle.
        if simulation_enabled() {
            self.simulate_and_log(&txs_base64).await;
        }

        // 6. Submit and map the response.
        let submitted_at = SystemTime::now();
        let t_send = Instant::now();
        let response = self
            .gateway
            .send_bundle(txs_base64)
            .await
            .map_err(|e| SubmitError::Transport(format!("send_bundle: {e}")))?;
        debug!(
            prepare_ms,
            send_ms = t_send.elapsed().as_millis(),
            total_ms = t_prepare.elapsed().as_millis(),
            "TIMING(submitter): bundle build+fetch vs network send to block engine"
        );

        match parse_send_response(&response) {
            Ok(bundle_id) => {
                info!(
                    bundle_id = %bundle_id,
                    tip_lamports = prep.tip_lamports,
                    slot = current_slot,
                    "bundle submitted"
                );
                Ok(BundleRecord {
                    bundle_id,
                    tip_lamports: prep.tip_lamports,
                    tip_account: prep.tip_account_str,
                    memo_signature: prep.built.memo_signature,
                    tip_signature: prep.built.tip_signature,
                    blockhash: prep.blockhash.to_string(),
                    blockhash_fetched_at_slot: current_slot,
                    submitted_at,
                    fault_injected: prep.fault_injected,
                })
            }
            Err(err) => {
                // Transport errors can embed the block-engine URL; redact it.
                warn!(error = %runtime::redact_url(&err.to_string()), slot = current_slot, "bundle submission failed");
                Err(err)
            }
        }
    }

    /// Resolve faults, fetch the (shared) blockhash + a tip account, and build +
    /// sign the two-transaction bundle. Shared by [`submit`](Self::submit) and
    /// [`simulate_bundle`](Self::simulate_bundle).
    async fn prepare(
        &self,
        spec: &BundleSpec,
        current_slot: u64,
    ) -> Result<Prepared, SubmitError> {
        // (`mut` is only exercised with the `fault-injection` feature.)
        #[allow(unused_mut)]
        let mut tip_lamports = spec.tip_lamports;
        #[allow(unused_mut)]
        let mut finalized = false;
        #[allow(unused_mut)]
        let mut fault_injected: Option<String> = None;

        #[cfg(feature = "fault-injection")]
        if let Some(fault) = spec.fault {
            match fault {
                Fault::StaleBlockhash { age_slots } => {
                    finalized = true;
                    fault_injected = Some(format!(
                        "StaleBlockhash{{requested_age_slots={age_slots},actual=finalized_commitment}}"
                    ));
                }
                Fault::SubFloorTip { lamports } => {
                    tip_lamports = lamports;
                    fault_injected = Some(format!("SubFloorTip{{lamports={lamports}}}"));
                }
            }
        }

        let blockhash = self.resolve_blockhash(finalized, current_slot).await?;

        let TipAccountStrategy::Random = self.config.tip_account_strategy;
        let tip_account_str = self
            .gateway
            .random_tip_account()
            .await
            .map_err(|e| SubmitError::Transport(format!("random_tip_account: {e}")))?;
        let tip_account = Pubkey::from_str(&tip_account_str).map_err(|e| {
            SubmitError::BadResponse(format!("invalid tip account '{tip_account_str}': {e}"))
        })?;

        let memo = self.full_memo(&spec.memo_text);
        let built = build_bundle(
            &self.keypair,
            &memo,
            self.config.self_transfer_lamports,
            spec.priority_fee_microlamports,
            tip_lamports,
            &tip_account,
            blockhash,
        )
        .map_err(|e| SubmitError::BadResponse(format!("bundle construction failed: {e}")))?;

        Ok(Prepared {
            built,
            tip_account,
            tip_account_str,
            tip_lamports,
            blockhash,
            fault_injected,
        })
    }

    /// Resolve the blockhash for this submission. Hot path (non-fault): read the
    /// background-cached blockhash with NO RPC round-trip (logging its age in
    /// slots). The stale-blockhash fault (`finalized`) intentionally needs an old
    /// hash, so it always bypasses the cache and fetches via RPC.
    async fn resolve_blockhash(
        &self,
        finalized: bool,
        current_slot: u64,
    ) -> Result<Hash, SubmitError> {
        if !finalized {
            if let Some(cache) = &self.blockhash_cache {
                if let Some(cached) = cache.get() {
                    let age_slots = current_slot.saturating_sub(cached.fetched_at_slot);
                    debug!(
                        blockhash = %cached.hash,
                        age_slots,
                        "hot path: using background-cached blockhash (no RPC round-trip)"
                    );
                    return Ok(cached.hash);
                }
                debug!("blockhash cache not warm yet; falling back to an RPC fetch this once");
            }
        }
        self.gateway
            .latest_blockhash(finalized)
            .await
            .map_err(|e| SubmitError::Transport(format!("get_latest_blockhash: {e}")))
    }

    /// Build the bundle exactly as [`submit`](Self::submit) would, but simulate
    /// each transaction individually against the RPC instead of sending — the
    /// definitive "would this bundle abort in Jito's atomic simulation?" probe.
    pub async fn simulate_bundle(
        &self,
        spec: BundleSpec,
        current_slot: u64,
    ) -> Result<BundleSimulation, SubmitError> {
        let prep = self.prepare(&spec, current_slot).await?;
        let labels = ["memo+self-transfer", "tip-transfer"];
        let signatures = [
            prep.built.memo_signature.clone(),
            prep.built.tip_signature.clone(),
        ];
        let txs = [prep.built.memo_base64, prep.built.tip_base64];

        let mut transactions = Vec::with_capacity(txs.len());
        for (i, b64) in txs.iter().enumerate() {
            let result = self
                .gateway
                .simulate_transaction(b64)
                .await
                .map_err(|e| SubmitError::Transport(format!("simulate {}: {e}", labels[i])))?;
            transactions.push(TxSimulation {
                label: labels[i],
                signature: signatures[i].clone(),
                result,
            });
        }

        Ok(BundleSimulation {
            blockhash: prep.blockhash.to_string(),
            tip_account: prep.tip_account_str,
            tip_lamports: prep.tip_lamports,
            transactions,
        })
    }

    /// Simulate each tx and log the result; warn loudly if any would abort.
    async fn simulate_and_log(&self, txs_base64: &[String]) {
        let labels = ["memo (tx0)", "tip-transfer (tx1)"];
        for (i, b64) in txs_base64.iter().enumerate() {
            let label = labels.get(i).copied().unwrap_or("tx");
            match self.gateway.simulate_transaction(b64).await {
                Ok(sim) => match &sim.err {
                    Some(err) => warn!(
                        tx = label,
                        error = %err,
                        units_consumed = ?sim.units_consumed,
                        logs = ?sim.logs,
                        "PRE-SUBMIT SIMULATION FAILED — this tx would abort, dropping the whole atomic bundle"
                    ),
                    None => debug!(
                        tx = label,
                        units_consumed = ?sim.units_consumed,
                        logs = ?sim.logs,
                        "PRE-SUBMIT SIMULATION ok"
                    ),
                },
                Err(e) => warn!(
                    tx = label,
                    error = %runtime::redact_url(&e.to_string()),
                    "PRE-SUBMIT SIMULATION request failed"
                ),
            }
        }
    }
}

/// Built bundle + the metadata needed to send or simulate it.
struct Prepared {
    built: BuiltBundle,
    tip_account: Pubkey,
    tip_account_str: String,
    tip_lamports: u64,
    blockhash: Hash,
    fault_injected: Option<String>,
}

/// Whether the debug-gated pre-submission simulation is enabled
/// (`SIMULATE_BEFORE_SEND=1` / `true`).
fn simulation_enabled() -> bool {
    std::env::var("SIMULATE_BEFORE_SEND")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests (offline)
// ---------------------------------------------------------------------------
//
// Construction is pure; the network is mocked behind `BundleGateway`. No live
// submission — the first real send happens later via the orchestrator.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn test_config() -> SubmitterConfig {
        SubmitterConfig {
            block_engine_url: "https://example.invalid/api/v1".to_string(),
            memo_prefix: "stx:".to_string(),
            tip_account_strategy: TipAccountStrategy::Random,
            self_transfer_lamports: 1_000,
            jito_rps: 1,
            auth_uuid: None,
        }
    }

    fn ok_response(id: &str) -> serde_json::Value {
        serde_json::json!({ "jsonrpc": "2.0", "result": id, "id": 1 })
    }

    // --- memo program id ---

    #[test]
    fn memo_program_id_is_valid() {
        // Parses to a 32-byte pubkey and matches the canonical SPL Memo id.
        let id = memo_program_id();
        assert_eq!(id.to_string(), MEMO_PROGRAM_ID);
        assert_eq!(id.to_string(), "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
    }

    // --- compute budget / priority fee ---

    #[test]
    fn compute_budget_program_id_is_valid() {
        let id = compute_budget_program_id();
        assert_eq!(id.to_string(), COMPUTE_BUDGET_PROGRAM_ID);
        assert_eq!(id.to_string(), "ComputeBudget111111111111111111111111111111");
    }

    #[test]
    fn compute_unit_price_instruction_encoding() {
        // SetComputeUnitPrice = discriminant 3 + u64 LE micro-lamports, no accounts.
        let ix = compute_unit_price_instruction(12_345);
        assert_eq!(ix.program_id, compute_budget_program_id());
        assert!(ix.accounts.is_empty());
        assert_eq!(ix.data.len(), 9);
        assert_eq!(ix.data[0], 3);
        assert_eq!(&ix.data[1..9], &12_345u64.to_le_bytes());
    }

    #[test]
    fn build_bundle_adds_priority_fee_only_when_nonzero() {
        let kp = Keypair::new();
        let tip_account = Pubkey::new_unique();
        let blockhash = Hash::new_unique();

        // priority_fee = 0 -> tx0 is just [memo, self-transfer]; no compute budget.
        let plain = build_bundle(&kp, "m", 1_000, 0, 5_000, &tip_account, blockhash).unwrap();
        let tx0 = decode_bundle_tx(&plain.memo_base64).unwrap();
        assert_eq!(tx0.instructions.len(), 2);
        assert_eq!(tx0.instructions[0].program_id, memo_program_id());
        assert!(tx0
            .instructions
            .iter()
            .all(|ix| ix.program_id != compute_budget_program_id()));

        // priority_fee > 0 -> tx0 = [compute-budget price, memo, self-transfer].
        let bam = build_bundle(&kp, "m", 1_000, 7_500, 5_000, &tip_account, blockhash).unwrap();
        let tx0 = decode_bundle_tx(&bam.memo_base64).unwrap();
        assert_eq!(tx0.instructions.len(), 3);
        assert_eq!(tx0.instructions[0].program_id, compute_budget_program_id());
        assert_eq!(tx0.instructions[0].data_len, 9);
        assert_eq!(tx0.instructions[1].program_id, memo_program_id());
        // The tip tx is unchanged regardless of priority fee.
        assert!(bundle_has_valid_tip(
            &[bam.memo_base64.clone(), bam.tip_base64.clone()],
            &tip_account,
            5_000,
            &kp.pubkey()
        ));
    }

    // --- pure construction ---

    #[test]
    fn build_bundle_two_signed_txs_share_blockhash() {
        let kp = Keypair::new();
        let tip_account = Pubkey::new_unique();
        let blockhash = Hash::new_unique();

        let built = build_bundle(&kp, "hello", 1_000, 0, 10_000, &tip_account, blockhash).unwrap();

        // Both txs anchored on the same blockhash.
        assert_eq!(built.memo_tx.message.recent_blockhash, blockhash);
        assert_eq!(built.tip_tx.message.recent_blockhash, blockhash);

        // Both correctly signed by the fee payer (exactly one signature each).
        assert_eq!(built.memo_tx.signatures.len(), 1);
        assert_eq!(built.tip_tx.signatures.len(), 1);
        built.memo_tx.verify().expect("memo tx signature valid");
        built.tip_tx.verify().expect("tip tx signature valid");

        // tx1 = memo (instr 0) + self-transfer (instr 1) — two instructions.
        let memo_msg = &built.memo_tx.message;
        assert_eq!(memo_msg.instructions.len(), 2);
        let memo_ci = &memo_msg.instructions[0];
        let memo_prog = memo_msg.account_keys[memo_ci.program_id_index as usize];
        assert_eq!(memo_prog, memo_program_id());
        assert_eq!(memo_ci.data, b"hello");

        // The self-transfer (instr 1) is a system transfer of wallet -> wallet.
        let st_ci = &memo_msg.instructions[1];
        let st_prog = memo_msg.account_keys[st_ci.program_id_index as usize];
        assert_eq!(st_prog, Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap());
        let st_from = memo_msg.account_keys[st_ci.accounts[0] as usize];
        let st_to = memo_msg.account_keys[st_ci.accounts[1] as usize];
        assert_eq!(st_from, kp.pubkey(), "self-transfer source is the wallet");
        assert_eq!(st_to, kp.pubkey(), "self-transfer destination is the wallet");
        assert_eq!(&st_ci.data[0..4], &2u32.to_le_bytes()); // SystemInstruction::Transfer
        assert_eq!(&st_ci.data[4..12], &1_000u64.to_le_bytes());
    }

    #[test]
    fn tip_instruction_targets_account_with_exact_lamports() {
        let kp = Keypair::new();
        let tip_account = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let tip = 12_345u64;

        let built = build_bundle(&kp, "m", 1_000, 0, tip, &tip_account, blockhash).unwrap();

        let msg = &built.tip_tx.message;
        let ci = &msg.instructions[0];
        // Program is the system program.
        let prog = msg.account_keys[ci.program_id_index as usize];
        assert_eq!(prog, Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap());
        // Destination (2nd account of a transfer) is our chosen tip account.
        let to = msg.account_keys[ci.accounts[1] as usize];
        assert_eq!(to, tip_account);
        // Source is the fee payer.
        let from = msg.account_keys[ci.accounts[0] as usize];
        assert_eq!(from, kp.pubkey());
        // SystemInstruction::Transfer = enum index 2 (u32 LE) + lamports (u64 LE).
        assert_eq!(&ci.data[0..4], &2u32.to_le_bytes());
        assert_eq!(&ci.data[4..12], &tip.to_le_bytes());
    }

    #[test]
    fn diagnostic_decodes_fully_built_bundle_and_finds_tip() {
        let kp = Keypair::new();
        let payer = kp.pubkey();
        let tip_account = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let tip = 12_623u64; // the lamport amount from the failing live run

        let built = build_bundle(&kp, "stx:diag", 1_000, 0, tip, &tip_account, blockhash).unwrap();
        // Decode exactly what gets sent (base64 -> bincode -> Transaction).
        let txs = vec![built.memo_base64.clone(), built.tip_base64.clone()];

        let tx0 = decode_bundle_tx(&txs[0]).unwrap(); // memo + self-transfer
        let tx1 = decode_bundle_tx(&txs[1]).unwrap(); // tip (LAST)

        // Print the full structure for inspection.
        eprintln!("tx0 (memo+self-transfer): payer={} blockhash={} sigs={} instrs={}", tx0.fee_payer, tx0.recent_blockhash, tx0.num_signatures, tx0.instructions.len());
        for ix in &tx0.instructions {
            eprintln!("  program={} accounts={:?} data_len={} transfer={:?}", ix.program_id, ix.accounts, ix.data_len, ix.transfer);
        }
        eprintln!("tx1 (tip): payer={} blockhash={} sigs={} instrs={}", tx1.fee_payer, tx1.recent_blockhash, tx1.num_signatures, tx1.instructions.len());
        for ix in &tx1.instructions {
            eprintln!("  program={} accounts={:?} data_len={} transfer={:?}", ix.program_id, ix.accounts, ix.data_len, ix.transfer);
        }

        // tx0 = memo (no transfer) + self-transfer (wallet -> wallet, real content).
        assert_eq!(tx0.instructions.len(), 2);
        assert_eq!(tx0.instructions[0].program_id, memo_program_id());
        assert!(tx0.instructions[0].transfer.is_none());
        assert_eq!(tx0.instructions[1].program_id, system_program_id());
        let st = tx0.instructions[1]
            .transfer
            .expect("tx0 must contain the self-transfer");
        assert_eq!(st.from, payer, "self-transfer from our wallet");
        assert_eq!(st.to, payer, "self-transfer back to our own wallet");
        assert_eq!(st.lamports, 1_000);

        // tx1 (LAST) is the tip transfer: system program, exact source/dest/amount.
        assert_eq!(tx1.instructions.len(), 1);
        assert_eq!(tx1.instructions[0].program_id, system_program_id());
        let t = tx1.instructions[0].transfer.expect("tip tx must contain a SystemProgram::transfer");
        assert_eq!(t.from, payer, "tip funded from our wallet");
        assert_eq!(t.to, tip_account, "tip sent to the fetched tip account");
        assert_eq!(t.lamports, tip, "tip amount equals tip_lamports");

        // Both txs share the recent blockhash, and both are signed once.
        assert_eq!(tx0.recent_blockhash, blockhash);
        assert_eq!(tx1.recent_blockhash, blockhash);
        assert_eq!(tx0.num_signatures, 1);
        assert_eq!(tx1.num_signatures, 1);

        // End-to-end verdict.
        assert!(bundle_has_valid_tip(&txs, &tip_account, tip, &payer));
    }

    #[test]
    fn diagnostic_flags_sub_minimum_tip_and_wrong_destination() {
        let kp = Keypair::new();
        let payer = kp.pubkey();
        let tip_account = Pubkey::new_unique();
        let blockhash = Hash::new_unique();

        // Sub-1000 tip -> not a valid Jito tip even though structurally a transfer.
        let low = build_bundle(&kp, "m", 1_000, 0, 500, &tip_account, blockhash).unwrap();
        let low_txs = vec![low.memo_base64, low.tip_base64];
        assert!(!bundle_has_valid_tip(&low_txs, &tip_account, 500, &payer));
        // The transfer is still decodable (proving the check is min-aware, not blind).
        assert_eq!(
            decode_bundle_tx(&low_txs[1]).unwrap().instructions[0]
                .transfer
                .unwrap()
                .lamports,
            500
        );

        // Wrong destination -> not detected as a valid tip to the expected account.
        let good = build_bundle(&kp, "m", 1_000, 0, 5_000, &tip_account, blockhash).unwrap();
        let good_txs = vec![good.memo_base64, good.tip_base64];
        let some_other_account = Pubkey::new_unique();
        assert!(!bundle_has_valid_tip(&good_txs, &some_other_account, 5_000, &payer));
        assert!(bundle_has_valid_tip(&good_txs, &tip_account, 5_000, &payer));
    }

    #[test]
    fn memo_instruction_matches_spl_memo_canonical_output() {
        use solana_sdk::instruction::AccountMeta;
        // spl_memo::build_memo(memo, signer_pubkeys) builds:
        //   Instruction { program_id: spl_memo::id(), accounts: signer_pubkeys
        //   .map(|pk| AccountMeta::new_readonly(*pk, true)), data: memo.to_vec() }
        // With no required signers (signer_pubkeys = &[]) the SPL Memo program
        // accepts an EMPTY account list — this is the canonical no-signer memo.
        let memo_text = "stx:bundle-7";
        let ours = memo_instruction(memo_text);

        // Replicated spl-memo build_memo(memo, &[]):
        let canonical = Instruction {
            program_id: Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr").unwrap(),
            accounts: Vec::<AccountMeta>::new(),
            data: memo_text.as_bytes().to_vec(),
        };

        assert_eq!(ours.program_id, canonical.program_id);
        assert_eq!(ours.accounts, canonical.accounts); // both empty
        assert_eq!(ours.data, canonical.data); // raw UTF-8 memo bytes
        assert_eq!(ours.accounts.len(), 0, "no-signer memo has no account metas");
    }

    #[test]
    fn memo_prefix_is_applied() {
        let kp = Keypair::new();
        let submitter = BundleSubmitter::with_gateway(test_config(), MockGateway::ok(), kp);
        assert_eq!(submitter.full_memo("body"), "stx:body");
    }

    // --- error mapping ---

    #[test]
    fn parse_response_success() {
        assert_eq!(parse_send_response(&ok_response("abc123")).unwrap(), "abc123");
    }

    #[test]
    fn parse_response_rejected() {
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "error": { "code": -32602, "message": "tip below floor" },
            "id": 1
        });
        assert_eq!(
            parse_send_response(&resp).unwrap_err(),
            SubmitError::Rejected { reason: "tip below floor".to_string() }
        );
    }

    #[test]
    fn parse_response_bad() {
        let resp = serde_json::json!({ "jsonrpc": "2.0", "id": 1 });
        assert!(matches!(
            parse_send_response(&resp).unwrap_err(),
            SubmitError::BadResponse(_)
        ));
    }

    // --- getTipAccounts parsing / selection ---

    #[test]
    fn parse_tip_accounts_valid_shape() {
        // The verified-working response shape (8 base58 strings).
        let body = r#"{"jsonrpc":"2.0","result":[
            "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
            "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
            "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
            "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
            "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
            "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
            "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
            "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT"
        ],"id":1}"#;
        let accounts = parse_tip_accounts(body).unwrap();
        assert_eq!(accounts.len(), 8);
        assert!(accounts[0].starts_with("96gY"));
    }

    #[test]
    fn parse_tip_accounts_empty_body_is_decode_error() {
        // Exactly the mainnet symptom: empty body -> serde EOF -> "error decoding
        // response body", which the failure classifier maps to TransportError.
        let err = parse_tip_accounts("").unwrap_err().to_string();
        assert!(err.contains("error decoding response body"), "got: {err}");
        assert!(err.contains("EOF while parsing"), "got: {err}");
    }

    #[test]
    fn parse_tip_accounts_missing_result_errors() {
        assert!(parse_tip_accounts(r#"{"jsonrpc":"2.0","id":1}"#).is_err());
        // Present but empty array.
        assert!(parse_tip_accounts(r#"{"result":[],"id":1}"#).is_err());
    }

    #[test]
    fn choose_account_picks_from_list_and_handles_empty() {
        let accounts = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let chosen = choose_account(&accounts).unwrap();
        assert!(accounts.contains(&chosen));
        assert_eq!(choose_account(&[]), None);
    }

    #[test]
    fn tip_accounts_url_appends_api_v1_to_bare_host() {
        // The configured base is the bare host (no /api/v1); the submitter adds
        // /api/v1 itself, matching the SDK base used for sendBundle.
        let gw = LiveGateway::new(
            solana_client::nonblocking::rpc_client::RpcClient::new("http://localhost:8899".into()),
            "https://mainnet.block-engine.jito.wtf",
            1,
            None,
        );
        assert_eq!(
            gw.tip_accounts_url,
            "https://mainnet.block-engine.jito.wtf/api/v1/getTipAccounts"
        );

        // A trailing slash on the base is trimmed before appending /api/v1/....
        let gw2 = LiveGateway::new(
            solana_client::nonblocking::rpc_client::RpcClient::new("http://localhost:8899".into()),
            "https://mainnet.block-engine.jito.wtf/",
            1,
            None,
        );
        assert_eq!(
            gw2.tip_accounts_url,
            "https://mainnet.block-engine.jito.wtf/api/v1/getTipAccounts"
        );
        // The status endpoints live under the same /api/v1 base.
        assert_eq!(
            gw2.inflight_statuses_url,
            "https://mainnet.block-engine.jito.wtf/api/v1/getInflightBundleStatuses"
        );
        assert_eq!(
            gw2.bundle_statuses_url,
            "https://mainnet.block-engine.jito.wtf/api/v1/getBundleStatuses"
        );
    }

    // --- rate limiter ---

    #[tokio::test]
    async fn rate_limiter_paces_sequential_acquires() {
        // 50 rps => 20ms interval. First token is immediate; each subsequent one
        // is spaced ~one interval, so three acquires take ~2 intervals (~40ms).
        let lim = JitoRateLimiter::new(50);
        let start = std::time::Instant::now();
        lim.acquire(Priority::High).await;
        lim.acquire(Priority::Low).await;
        lim.acquire(Priority::Low).await;
        assert!(
            start.elapsed() >= Duration::from_millis(30),
            "expected >=2 intervals of pacing, got {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn rate_limiter_low_yields_while_high_pending() {
        // While a High acquisition is pending, a Low caller must back off (it
        // sleeps ~one interval per loop). Clear the High after ~one interval and
        // confirm the Low waited rather than grabbing the slot immediately.
        let lim = Arc::new(JitoRateLimiter::new(20)); // 50ms interval
        lim.high_pending.fetch_add(1, Ordering::SeqCst);
        let releaser = Arc::clone(&lim);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(60)).await;
            releaser.high_pending.fetch_sub(1, Ordering::SeqCst);
        });
        let start = std::time::Instant::now();
        lim.acquire(Priority::Low).await;
        assert!(
            start.elapsed() >= Duration::from_millis(50),
            "Low should yield ~1 interval to the pending High, waited {:?}",
            start.elapsed()
        );
    }

    // --- bundle status parsing ---

    #[test]
    fn parse_inflight_status_maps_each_state() {
        let mk = |s: &str| {
            format!(
                r#"{{"jsonrpc":"2.0","result":{{"context":{{"slot":280}},
                "value":[{{"bundle_id":"abc","status":"{s}","landed_slot":null}}]}},"id":1}}"#
            )
        };
        assert_eq!(parse_inflight_status(&mk("Pending"), "abc").unwrap(), BundleStatus::Pending);
        assert_eq!(parse_inflight_status(&mk("Failed"), "abc").unwrap(), BundleStatus::Failed);
        assert_eq!(parse_inflight_status(&mk("Landed"), "abc").unwrap(), BundleStatus::Landed);
        assert_eq!(parse_inflight_status(&mk("Invalid"), "abc").unwrap(), BundleStatus::Invalid);
    }

    #[test]
    fn parse_inflight_status_missing_id_is_invalid() {
        // Our id absent from a (possibly empty) value array => not in system.
        let empty = r#"{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":[]},"id":1}"#;
        assert_eq!(parse_inflight_status(empty, "abc").unwrap(), BundleStatus::Invalid);
        // A `null` value array (Jito's literal "unknown" shape) => Invalid too.
        let nullv = r#"{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":null},"id":1}"#;
        assert_eq!(parse_inflight_status(nullv, "abc").unwrap(), BundleStatus::Invalid);
    }

    #[test]
    fn parse_inflight_status_surfaces_rpc_error() {
        let err = r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"bad"},"id":1}"#;
        assert!(parse_inflight_status(err, "abc").is_err());
    }

    #[test]
    fn parse_bundle_detail_extracts_slot_and_signatures() {
        let body = r#"{"jsonrpc":"2.0","result":{"context":{"slot":290},"value":[
            {"bundle_id":"abc","transactions":["sig1","sig2"],"slot":285,
             "confirmation_status":"finalized","err":{"Ok":null}}]},"id":1}"#;
        let (slot, conf, sigs) = parse_bundle_detail(body, "abc").unwrap();
        assert_eq!(slot, Some(285));
        assert_eq!(conf.as_deref(), Some("finalized"));
        assert_eq!(sigs, vec!["sig1".to_string(), "sig2".to_string()]);
    }

    #[test]
    fn parse_bundle_detail_missing_entry_is_empty() {
        let body = r#"{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":[null]},"id":1}"#;
        let (slot, conf, sigs) = parse_bundle_detail(body, "abc").unwrap();
        assert_eq!(slot, None);
        assert!(conf.is_none());
        assert!(sigs.is_empty());
    }

    #[test]
    fn bundle_status_terminal_classification() {
        assert!(BundleStatus::Landed.is_terminal());
        assert!(BundleStatus::Failed.is_terminal());
        assert!(BundleStatus::Invalid.is_terminal());
        assert!(!BundleStatus::Pending.is_terminal());
        assert!(!BundleStatus::Unknown("x".into()).is_terminal());
    }

    // --- mock gateway ---

    #[derive(Default)]
    struct Captured {
        finalized: Option<bool>,
        sent_txs: Option<Vec<String>>,
    }

    struct MockGateway {
        blockhash: Hash,
        finalized_blockhash: Hash,
        tip_account: String,
        send_response: Option<serde_json::Value>, // None => simulate transport error
        blockhash_err: bool,
        captured: Mutex<Captured>,
    }

    impl MockGateway {
        fn ok() -> Self {
            Self {
                blockhash: Hash::new_unique(),
                finalized_blockhash: Hash::new_unique(),
                tip_account: Pubkey::new_unique().to_string(),
                send_response: Some(ok_response("bundle-xyz")),
                blockhash_err: false,
                captured: Mutex::new(Captured::default()),
            }
        }
    }

    impl BundleGateway for MockGateway {
        async fn latest_blockhash(&self, finalized: bool) -> anyhow::Result<Hash> {
            self.captured.lock().unwrap().finalized = Some(finalized);
            if self.blockhash_err {
                anyhow::bail!("simulated rpc failure");
            }
            Ok(if finalized {
                self.finalized_blockhash
            } else {
                self.blockhash
            })
        }

        async fn random_tip_account(&self) -> anyhow::Result<String> {
            Ok(self.tip_account.clone())
        }

        async fn warm_tip_cache(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_bundle(&self, txs_base64: Vec<String>) -> anyhow::Result<serde_json::Value> {
            self.captured.lock().unwrap().sent_txs = Some(txs_base64);
            match &self.send_response {
                Some(v) => Ok(v.clone()),
                None => anyhow::bail!("simulated transport failure"),
            }
        }

        async fn simulate_transaction(&self, _tx_base64: &str) -> anyhow::Result<SimulationResult> {
            Ok(SimulationResult {
                err: None,
                logs: vec!["Program log: mock".to_string()],
                units_consumed: Some(150),
            })
        }

        async fn poll_bundle_status(&self, bundle_id: &str) -> anyhow::Result<BundleStatusReport> {
            Ok(BundleStatusReport {
                bundle_id: bundle_id.to_string(),
                status: BundleStatus::Pending,
                landed_slot: None,
                confirmation_status: None,
                signatures: Vec::new(),
            })
        }
    }

    fn decode_tx(b64: &str) -> Transaction {
        bincode::deserialize(&BASE64.decode(b64).unwrap()).unwrap()
    }

    fn spec(tip: u64, memo: &str) -> BundleSpec {
        BundleSpec {
            tip_lamports: tip,
            memo_text: memo.to_string(),
            priority_fee_microlamports: 0,
            #[cfg(feature = "fault-injection")]
            fault: None,
        }
    }

    #[tokio::test]
    async fn submit_happy_path_records_metadata() {
        let kp = Keypair::new();
        let payer = kp.pubkey();
        let gw = MockGateway::ok();
        let expected_tip_account = gw.tip_account.clone();
        let expected_blockhash = gw.blockhash.to_string();
        let submitter = BundleSubmitter::with_gateway(test_config(), gw, kp);

        let record = submitter.submit(spec(50_000, "hi"), 4242).await.unwrap();

        assert_eq!(record.bundle_id, "bundle-xyz");
        assert_eq!(record.tip_lamports, 50_000);
        assert_eq!(record.tip_account, expected_tip_account);
        assert_eq!(record.blockhash, expected_blockhash);
        assert_eq!(record.blockhash_fetched_at_slot, 4242);
        assert!(record.fault_injected.is_none());

        // The two transactions actually sent: memo (prefix applied) + tip.
        let captured = submitter.gateway.captured.lock().unwrap();
        let sent = captured.sent_txs.as_ref().unwrap();
        assert_eq!(sent.len(), 2);
        let memo_tx = decode_tx(&sent[0]);
        assert_eq!(memo_tx.message.instructions[0].data, b"stx:hi");
        let tip_tx = decode_tx(&sent[1]);
        let to = tip_tx.message.account_keys[tip_tx.message.instructions[0].accounts[1] as usize];
        assert_eq!(to.to_string(), expected_tip_account);
        assert_eq!(tip_tx.message.account_keys[0], payer);
        // Confirmed commitment (not finalized) on the happy path.
        assert_eq!(captured.finalized, Some(false));
    }

    #[tokio::test]
    async fn submit_transport_error_on_send() {
        let kp = Keypair::new();
        let mut gw = MockGateway::ok();
        gw.send_response = None; // force send failure
        let submitter = BundleSubmitter::with_gateway(test_config(), gw, kp);

        let err = submitter.submit(spec(1_000, "x"), 1).await.unwrap_err();
        assert!(matches!(err, SubmitError::Transport(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn submit_transport_error_on_blockhash() {
        let kp = Keypair::new();
        let mut gw = MockGateway::ok();
        gw.blockhash_err = true;
        let submitter = BundleSubmitter::with_gateway(test_config(), gw, kp);

        let err = submitter.submit(spec(1_000, "x"), 1).await.unwrap_err();
        assert!(matches!(err, SubmitError::Transport(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn submit_rejected_maps_reason() {
        let kp = Keypair::new();
        let mut gw = MockGateway::ok();
        gw.send_response = Some(serde_json::json!({
            "jsonrpc": "2.0",
            "error": { "code": -32602, "message": "tip 500 below floor" },
            "id": 1
        }));
        let submitter = BundleSubmitter::with_gateway(test_config(), gw, kp);

        let err = submitter.submit(spec(500, "x"), 1).await.unwrap_err();
        assert_eq!(
            err,
            SubmitError::Rejected { reason: "tip 500 below floor".to_string() }
        );
    }

    // --- fault injection (feature-gated) ---

    #[cfg(feature = "fault-injection")]
    #[tokio::test]
    async fn fault_sub_floor_tip_overrides_amount() {
        let kp = Keypair::new();
        let gw = MockGateway::ok();
        let submitter = BundleSubmitter::with_gateway(test_config(), gw, kp);

        let spec = BundleSpec {
            tip_lamports: 50_000,
            memo_text: "x".to_string(),
            priority_fee_microlamports: 0,
            fault: Some(Fault::SubFloorTip { lamports: 500 }),
        };
        let record = submitter.submit(spec, 7).await.unwrap();

        assert_eq!(record.tip_lamports, 500);
        assert_eq!(record.fault_injected.as_deref(), Some("SubFloorTip{lamports=500}"));

        // The actually-sent tip tx transfers the sub-floor amount.
        let captured = submitter.gateway.captured.lock().unwrap();
        let tip_tx = decode_tx(&captured.sent_txs.as_ref().unwrap()[1]);
        assert_eq!(&tip_tx.message.instructions[0].data[4..12], &500u64.to_le_bytes());
    }

    #[cfg(feature = "fault-injection")]
    #[tokio::test]
    async fn fault_stale_blockhash_uses_finalized() {
        let kp = Keypair::new();
        let gw = MockGateway::ok();
        let finalized_hash = gw.finalized_blockhash.to_string();
        let submitter = BundleSubmitter::with_gateway(test_config(), gw, kp);

        let spec = BundleSpec {
            tip_lamports: 50_000,
            memo_text: "x".to_string(),
            priority_fee_microlamports: 0,
            fault: Some(Fault::StaleBlockhash { age_slots: 100 }),
        };
        let record = submitter.submit(spec, 9).await.unwrap();

        // Fetched at finalized commitment, recorded that hash, marked the record.
        assert_eq!(submitter.gateway.captured.lock().unwrap().finalized, Some(true));
        assert_eq!(record.blockhash, finalized_hash);
        assert!(record.fault_injected.unwrap().starts_with("StaleBlockhash{"));
    }
}
