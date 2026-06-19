//! The wired-together running system: shared handles, the submission pipeline,
//! the agent retry loop, the timeout sweeper, and the run modes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context as _;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tracing::{debug, info, warn};

use agent::{
    clamp_actions, Action, AgentConfig, AgentKind, BaselineAgent, DecisionAgent, DecisionContext,
    DecisionRecord, FailureReasoningAgent, PriorAttempt,
};
use leader::{LeaderConfig, LeaderTracker, RpcJitoSource};
use lifecycle::{LifecycleConfig, LifecycleTracker};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use stream::{StreamClient, StreamConfig, StreamEvent};
use submitter::{
    BlockhashCache, BundleRecord, BundleSpec, BundleSubmitter, LiveGateway, SubmitError,
    SubmitterConfig, TipAccountStrategy,
};
use tips::{TipConfig, TipSnapshot, TipTracker, TipTrend};

use crate::config::Config;

/// Trend window the agent reasons over.
const TREND_WINDOW: Duration = Duration::from_secs(60);
/// Memo prefix stamped on every bundle.
const MEMO_PREFIX: &str = "stx:";
/// Stream event channel depth.
const EVENT_CHANNEL_CAPACITY: usize = 1024;
/// Approximate slot duration (mainnet) — used to pace the timeout sweeper.
const SLOT_MILLIS: u64 = 400;
/// How often the background task refreshes the cached blockhash. A blockhash is
/// valid ~150 slots (~60s), so 2s keeps it fresh with huge validity runway while
/// keeping the blockhash RPC out of the submission hot path.
const BLOCKHASH_REFRESH: Duration = Duration::from_secs(2);
/// How often the background task re-fetches the Jito tip accounts. Comfortably
/// under the 5-min cache TTL so the hot path always reads a fresh cache.
const TIP_REFRESH: Duration = Duration::from_secs(120);

/// Jito bundle-status poll cadence and budget. After an accepted bundle we poll
/// `getInflightBundleStatuses` every `BUNDLE_STATUS_POLL` for up to
/// `BUNDLE_STATUS_BUDGET`, logging each status — an authoritative off-chain
/// signal that runs alongside on-chain lifecycle reconciliation.
// 5s (not 2s) keeps the background poller off the 1-RPS Jito budget that a live
// sendBundle needs — fewer self-inflicted getInflightBundleStatuses calls.
const BUNDLE_STATUS_POLL: Duration = Duration::from_secs(5);
const BUNDLE_STATUS_BUDGET: Duration = Duration::from_secs(30);

/// Unique-ifies synthetic rejection rows (rejected bundles never produce a real
/// signature / bundle id, but the lifecycle row needs unique keys).
static REJECTION_SEQ: AtomicU64 = AtomicU64::new(0);

/// Poll Jito's `getInflightBundleStatuses` for an accepted `bundle_id` and log
/// every status transition loudly, until a terminal verdict or the time budget
/// runs out. Detached background task — this is a *diagnostic* signal straight
/// from Jito (does the bundle enter the auction and Pending→Landed/Failed, or is
/// it Invalid/never-entered?), independent of the on-chain lifecycle tracker.
fn spawn_bundle_status_poll(
    submitter: Arc<BundleSubmitter<LiveGateway>>,
    lifecycle: LifecycleTracker,
    bundle_db_id: i64,
    bundle_id: String,
) {
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let mut last: Option<String> = None;
        loop {
            match submitter.bundle_status(&bundle_id).await {
                Ok(report) => {
                    let elapsed_ms = start.elapsed().as_millis();
                    let status = report.status.as_str().to_string();
                    // Persist EVERY observed verdict so a later never-landed timeout
                    // can classify AuctionLost (Jito Invalid) vs ExpiredBlockhash.
                    if let Err(err) = lifecycle.record_jito_status(bundle_db_id, &status).await {
                        warn!(
                            bundle_db_id,
                            error = %runtime::redact_url(&err.to_string()),
                            "BUNDLE STATUS: failed to persist Jito status"
                        );
                    }
                    // Log every poll the first time we see a status, then only on
                    // change, so transitions stand out without spamming Pending.
                    if last.as_deref() != Some(status.as_str()) {
                        info!(
                            bundle_id = %bundle_id,
                            status = %status,
                            landed_slot = ?report.landed_slot,
                            confirmation = ?report.confirmation_status,
                            signatures = ?report.signatures,
                            elapsed_ms,
                            "BUNDLE STATUS (Jito getInflightBundleStatuses)"
                        );
                        last = Some(status);
                    }
                    if report.status.is_terminal() {
                        info!(
                            bundle_id = %bundle_id,
                            final_status = %report.status.as_str(),
                            elapsed_ms,
                            "BUNDLE STATUS: terminal — Jito's authoritative verdict"
                        );
                        return;
                    }
                }
                Err(err) => warn!(
                    bundle_id = %bundle_id,
                    error = %runtime::redact_url(&err.to_string()),
                    "BUNDLE STATUS: poll failed; will retry"
                ),
            }
            if start.elapsed() >= BUNDLE_STATUS_BUDGET {
                warn!(
                    bundle_id = %bundle_id,
                    last_status = ?last,
                    budget_ms = BUNDLE_STATUS_BUDGET.as_millis(),
                    "BUNDLE STATUS: budget elapsed without a terminal verdict (still Pending?)"
                );
                return;
            }
            tokio::time::sleep(BUNDLE_STATUS_POLL).await;
        }
    });
}

/// Per-state bundle counts. Terminal = Finalized + Failed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StatusCounts {
    pub submitted: i64,
    pub processed: i64,
    pub confirmed: i64,
    pub finalized: i64,
    pub failed: i64,
    pub other: i64,
}

impl StatusCounts {
    /// Bundles not yet in a terminal state (Submitted/Processed/Confirmed).
    pub fn non_terminal(&self) -> i64 {
        self.submitted + self.processed + self.confirmed
    }
}

// ---------------------------------------------------------------------------
// Submit seam (mockable)
// ---------------------------------------------------------------------------

/// The submission seam — abstracted so the retry loop is unit-testable without
/// real RPC / block-engine calls.
pub trait Submit: Send + Sync {
    fn submit_bundle(
        &self,
        spec: BundleSpec,
        current_slot: u64,
    ) -> impl std::future::Future<Output = Result<BundleRecord, SubmitError>> + Send;
}

impl Submit for BundleSubmitter<LiveGateway> {
    async fn submit_bundle(
        &self,
        spec: BundleSpec,
        current_slot: u64,
    ) -> Result<BundleRecord, SubmitError> {
        // `submit` is the inherent method; this trait method just forwards.
        self.submit(spec, current_slot).await
    }
}

// ---------------------------------------------------------------------------
// Tip policy (pure)
// ---------------------------------------------------------------------------

/// Which landed-tip percentile the normal policy targets. Configured via the
/// `TIP_PERCENTILE` env var (`p50` | `p75` | `p95`, default `p75`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipPercentile {
    P50,
    P75,
    P95,
}

impl TipPercentile {
    /// Parse from `"p50" | "p75" | "p95"` (case-insensitive). `None` if invalid.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "p50" => Some(Self::P50),
            "p75" => Some(Self::P75),
            "p95" => Some(Self::P95),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::P50 => "p50",
            Self::P75 => "p75",
            Self::P95 => "p95",
        }
    }

    fn select(self, snapshot: &TipSnapshot) -> u64 {
        match self {
            Self::P50 => snapshot.p50_lamports,
            Self::P75 => snapshot.p75_lamports,
            Self::P95 => snapshot.p95_lamports,
        }
    }
}

/// Lamports used when the tip feed is stale or absent (we don't trust a stale
/// percentile, and 1_511-lamport p50 tips were losing every auction live).
const STALE_TIP_FALLBACK: u64 = 10_000;

/// Result of the normal-submission tip policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipPolicy {
    pub tip: u64,
    pub p50: Option<u64>,
    pub p75: Option<u64>,
    /// `"live"`, `"stale"`, or `"no-data"` — drives loud logging.
    pub source: &'static str,
}

/// Tip for a NORMAL submission: target the configured `percentile` of recently
/// landed tips (default p75), floored at 1_000 and capped by `max_tip` — fully
/// driven by live data, no hardcoded tip. If the feed is **stale or absent** we
/// don't trust it and fall back to [`STALE_TIP_FALLBACK`] (the caller logs this
/// loudly). The agent — not this function — owns tips on retries.
pub fn normal_tip_policy(
    snapshot: Option<TipSnapshot>,
    is_stale: bool,
    percentile: TipPercentile,
    max_tip: u64,
) -> TipPolicy {
    match snapshot {
        // Live, trustworthy data: target the chosen percentile.
        Some(s) if !is_stale => TipPolicy {
            tip: percentile.select(&s).max(1_000).min(max_tip),
            p50: Some(s.p50_lamports),
            p75: Some(s.p75_lamports),
            source: "live",
        },
        // Stale data present: keep its p50/p75 for context, but use the fallback
        // tip rather than a possibly-low stale percentile.
        Some(s) => TipPolicy {
            tip: STALE_TIP_FALLBACK.min(max_tip),
            p50: Some(s.p50_lamports),
            p75: Some(s.p75_lamports),
            source: "stale",
        },
        // No data at all.
        None => TipPolicy {
            tip: STALE_TIP_FALLBACK.min(max_tip),
            p50: None,
            p75: None,
            source: "no-data",
        },
    }
}

// ---------------------------------------------------------------------------
// Action execution plan (pure)
// ---------------------------------------------------------------------------

/// What a decision's ordered action list resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecPlan {
    pub refresh: bool,
    pub set_tip: Option<u64>,
    pub hold_slots: u64,
    pub terminal: Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    Resubmit,
    Abandon,
    /// No terminal action in the list.
    None,
}

/// Resolve an ordered action list. Modifiers (refresh, set_tip, hold) accumulate
/// in order; the first `Resubmit`/`Abandon` is terminal and stops processing
/// (so actions after it are ignored — a deliberate "execute in order" rule).
pub fn plan_actions(actions: &[Action]) -> ExecPlan {
    let mut plan = ExecPlan {
        refresh: false,
        set_tip: None,
        hold_slots: 0,
        terminal: Terminal::None,
    };
    for action in actions {
        match action {
            Action::RefreshBlockhash => plan.refresh = true,
            Action::SetTip(n) => plan.set_tip = Some(*n),
            Action::Hold { slots } => plan.hold_slots = plan.hold_slots.saturating_add(*slots),
            Action::Resubmit => {
                plan.terminal = Terminal::Resubmit;
                break;
            }
            Action::Abandon => {
                plan.terminal = Terminal::Abandon;
                break;
            }
        }
    }
    plan
}

// ---------------------------------------------------------------------------
// Agent retry loop (generic; the heart)
// ---------------------------------------------------------------------------

/// Tip context snapshot for building a [`DecisionContext`].
#[derive(Debug, Clone, Copy, Default)]
pub struct TipCtx {
    pub p50: Option<u64>,
    pub p75: Option<u64>,
    pub trend: Option<TipTrend>,
    pub age_secs: Option<u64>,
}

/// Loop tuning.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub max_attempts: u32,
    pub max_tip: u64,
    pub model: String,
}

/// Build a [`BundleSpec`] with no injected fault and no priority fee. The
/// priority fee (for BAM leaders) is set per-submission in `submit_one`.
pub fn base_spec(tip: u64, memo: String) -> BundleSpec {
    BundleSpec {
        tip_lamports: tip,
        memo_text: memo,
        priority_fee_microlamports: 0,
        priority_fee_cu_limit: 0,
        #[cfg(feature = "fault-injection")]
        fault: None,
    }
}

/// The agent retry loop. Given the failed attempt's classification + evidence,
/// it repeatedly asks the agent what to do, executes the actions, and resubmits
/// — until success, an `Abandon`, or the attempt cap.
///
/// On each iteration the executed decision is persisted (`executed=true`); when
/// the LLM decides, the baseline is also persisted as a shadow (`executed=false`)
/// for A/B comparison.
#[allow(clippy::too_many_arguments)]
pub async fn agent_retry_loop<S, A>(
    submitter: &S,
    llm: &A,
    baseline: &BaselineAgent,
    lifecycle: &LifecycleTracker,
    agent_log: &agent::AgentLog,
    pool: &SqlitePool,
    slot_clock: &Arc<AtomicU64>,
    cfg: &LoopConfig,
    bundle_db_id: i64,
    mut classification: failure::Classification,
    mut evidence: failure::Evidence,
    mut spec: BundleSpec,
    blockhash_fetched_at_slot: u64,
    mut tip_provider: impl FnMut() -> TipCtx,
) -> anyhow::Result<()>
where
    S: Submit,
    A: DecisionAgent,
{
    // `attempt` is the attempt that just failed and brought us here (1-based).
    let mut attempt: u32 = 1;

    while attempt < cfg.max_attempts {
        let next_attempt = attempt + 1;
        let tipc = tip_provider();
        let current_slot = slot_clock.load(Ordering::Relaxed);
        let prior_attempts = load_prior_attempts(pool, bundle_db_id).await?;

        let ctx = DecisionContext {
            bundle_db_id,
            classification: classification.clone(),
            evidence: evidence.clone(),
            blockhash_age_slots: current_slot.saturating_sub(blockhash_fetched_at_slot),
            tip_lamports: spec.tip_lamports,
            tip_p50_now: tipc.p50,
            tip_p75_now: tipc.p75,
            tip_trend: tipc.trend,
            tip_data_age_secs: tipc.age_secs,
            attempt: next_attempt,
            prior_attempts,
            current_slot,
        };
        let ctx_json = ctx.to_json().unwrap_or_default();

        // --- decide, with observable fallback to baseline ---
        let started = std::time::Instant::now();
        let (decision, kind, model, latency_ms) = match llm.decide(&ctx).await {
            Ok(d) => (
                d,
                AgentKind::Llm,
                Some(cfg.model.clone()),
                Some(started.elapsed().as_millis() as u64),
            ),
            Err(err) => {
                warn!(
                    attempt = next_attempt,
                    reason = %err,
                    "LLM decision failed — falling back to BaselineAgent (executed=baseline)"
                );
                (baseline.decide_sync(&ctx), AgentKind::Baseline, None, None)
            }
        };

        // Persist the executed decision...
        agent_log
            .record_decision(&DecisionRecord::new(
                bundle_db_id,
                next_attempt,
                kind,
                ctx_json.clone(),
                &decision,
                model,
                latency_ms,
                true,
            ))
            .await?;
        // ...and the shadow baseline, but only when the LLM was the one executed.
        if matches!(kind, AgentKind::Llm) {
            let shadow = baseline.decide_sync(&ctx);
            agent_log
                .record_decision(&DecisionRecord::new(
                    bundle_db_id,
                    next_attempt,
                    AgentKind::Baseline,
                    ctx_json,
                    &shadow,
                    None,
                    None,
                    false,
                ))
                .await?;
        }

        // --- guardrail ---
        let (decision, clamped) = clamp_actions(&decision, cfg.max_tip);
        if clamped {
            warn!(
                attempt = next_attempt,
                max_tip = cfg.max_tip,
                "agent SetTip exceeded cap — clamped"
            );
        }
        info!(
            attempt = next_attempt,
            agent = ?kind,
            actions = ?decision.actions,
            rationale = %decision.rationale,
            "agent decision"
        );

        // --- execute actions in order ---
        let plan = plan_actions(&decision.actions);
        if let Some(tip) = plan.set_tip {
            spec.tip_lamports = tip;
        }
        if plan.hold_slots > 0 {
            info!(slots = plan.hold_slots, "agent: holding before resubmit");
            wait_slots(slot_clock, plan.hold_slots).await;
        }

        match plan.terminal {
            Terminal::Abandon => {
                info!(bundle_db_id, "agent chose Abandon; leaving bundle Failed");
                return Ok(());
            }
            Terminal::Resubmit | Terminal::None => {
                attempt = next_attempt;
                let slot = slot_clock.load(Ordering::Relaxed);
                match submitter.submit_bundle(spec.clone(), slot).await {
                    Ok(record) => {
                        let new_row = lifecycle
                            .record_submission(&record, tipc.p50, tipc.p75)
                            .await?;
                        info!(
                            bundle_db_id,
                            new_row,
                            bundle_id = %record.bundle_id,
                            tip = record.tip_lamports,
                            "resubmission accepted; now tracking"
                        );
                        return Ok(());
                    }
                    Err(err) => {
                        let raw = err.to_string();
                        warn!(
                            attempt,
                            error = %runtime::redact_url(&raw),
                            "resubmission rejected; re-deciding"
                        );
                        evidence = failure::Evidence::SubmitRejection {
                            raw_error: raw,
                        };
                        classification = failure::classify(&evidence);
                    }
                }
            }
        }
    }

    warn!(
        bundle_db_id,
        attempts = attempt,
        max = cfg.max_attempts,
        "attempt cap reached; abandoning bundle"
    );
    Ok(())
}

async fn load_prior_attempts(
    pool: &SqlitePool,
    bundle_db_id: i64,
) -> anyhow::Result<Vec<PriorAttempt>> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT attempt, rationale, actions_json FROM agent_decisions \
         WHERE bundle_db_id = ? AND executed = 1 ORDER BY attempt",
    )
    .bind(bundle_db_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(attempt, rationale, actions)| PriorAttempt {
            attempt: attempt as u32,
            decision_summary: format!("{rationale} -> {actions}"),
            outcome: "failed; retried".to_string(),
        })
        .collect())
}

/// Wait until the slot clock has advanced `n` slots from now.
pub async fn wait_slots(slot_clock: &Arc<AtomicU64>, n: u64) {
    if n == 0 {
        return;
    }
    let target = slot_clock.load(Ordering::Relaxed).saturating_add(n);
    while slot_clock.load(Ordering::Relaxed) < target {
        tokio::time::sleep(Duration::from_millis(SLOT_MILLIS / 2)).await;
    }
}

// ---------------------------------------------------------------------------
// App: shared handles + pipeline
// ---------------------------------------------------------------------------

/// The wired-together application. Shared by reference (often behind `Arc`).
pub struct App {
    pub config: Config,
    pub wallet_pubkey: Pubkey,
    pub slot_clock: Arc<AtomicU64>,
    pub leader: LeaderTracker<RpcJitoSource>,
    pub tips: TipTracker,
    pub submitter: Arc<BundleSubmitter<LiveGateway>>,
    pub lifecycle: LifecycleTracker,
    pub agent_log: agent::AgentLog,
    pub llm: FailureReasoningAgent,
    pub baseline: BaselineAgent,
    pub reconcile_rpc: Arc<solana_client::nonblocking::rpc_client::RpcClient>,
    pub blockhash_cache: BlockhashCache,
    pub pool: SqlitePool,
}

impl App {
    /// Build the full application from config: open the DB (migrations + hydrate),
    /// load the keypair, and construct every subsystem. Spawns nothing yet.
    pub async fn build(config: Config) -> anyhow::Result<Self> {
        let pool = open_pool(&config.db_path).await?;
        let lifecycle = LifecycleTracker::new(pool.clone(), LifecycleConfig::default()).await?;
        let agent_log = agent::AgentLog::new(pool.clone()).await?;

        // Keypair — log the pubkey only, never key material.
        let keypair = solana_sdk::signature::read_keypair_file(&config.wallet_keypair_path)
            .map_err(|e| anyhow::anyhow!("failed to read wallet keypair: {e}"))?;
        let wallet_pubkey = keypair.pubkey();
        info!(wallet = %wallet_pubkey, "loaded wallet keypair");

        // Shared slot clock + background-refreshed blockhash cache, so the
        // submission hot path reads the blockhash instantly (no RPC round-trip).
        let slot_clock = Arc::new(AtomicU64::new(0));
        let blockhash_cache = BlockhashCache::new();

        // Submitter (consumes its own RPC client + the keypair).
        let submitter_rpc =
            solana_client::nonblocking::rpc_client::RpcClient::new(config.rpc_url.clone());
        let submitter = Arc::new(
            BundleSubmitter::new(
                SubmitterConfig {
                    block_engine_url: config.jito_block_engine_url.clone(),
                    memo_prefix: MEMO_PREFIX.to_string(),
                    tip_account_strategy: TipAccountStrategy::Random,
                    self_transfer_lamports: config.self_transfer_lamports,
                    jito_rps: config.jito_rps,
                    auth_uuid: config.jito_auth_uuid.clone(),
                },
                submitter_rpc,
                keypair,
            )
            .with_blockhash_cache(blockhash_cache.clone()),
        );

        // A second RPC client for reconcile + the background blockhash refresher
        // (the submitter owns the first).
        let reconcile_rpc = Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new(
            config.rpc_url.clone(),
        ));

        let leader = LeaderTracker::new(
            LeaderConfig::default(),
            RpcJitoSource::mainnet(config.rpc_url.clone()),
        );
        let tips = TipTracker::live(TipConfig::default());

        let llm = FailureReasoningAgent::new(AgentConfig {
            api_key: config.anthropic_api_key.clone(),
            model: config.agent_model.clone(),
            max_tip_lamports: config.max_tip_lamports,
            request_timeout: config.agent_timeout,
        });

        Ok(Self {
            config,
            wallet_pubkey,
            slot_clock,
            leader,
            tips,
            submitter,
            lifecycle,
            agent_log,
            llm,
            baseline: BaselineAgent,
            reconcile_rpc,
            blockhash_cache,
            pool,
        })
    }

    /// Spawn all background tasks: stream → fan-out, tips, leader refresh,
    /// reconnect-driven reconcile, and the timeout sweeper. Returns the spawned
    /// task handles (dropped on shutdown).
    pub fn spawn_background(self: &Arc<Self>) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = Vec::new();

        // Stream → bounded channel, with a reconnect-notification channel.
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<StreamEvent>(EVENT_CHANNEL_CAPACITY);
        let (reconnect_tx, reconnect_rx) = tokio::sync::mpsc::channel::<u64>(8);

        let stream_cfg = StreamConfig {
            endpoint: self.config.yellowstone_endpoint.clone(),
            x_token: self.config.yellowstone_x_token.clone(),
        };
        let pubkeys = vec![self.wallet_pubkey];
        handles.push(tokio::spawn(async move {
            if let Err(err) =
                StreamClient::run_with_reconnect(stream_cfg, pubkeys, event_tx, reconnect_tx).await
            {
                warn!(error = %err, "stream supervisor exited");
            }
        }));

        // Fan-out: ONE task drains the channel and forwards each event to the
        // leader tracker and the lifecycle tracker (both cheap sync ingests),
        // and advances the orchestrator slot clock.
        {
            let leader = self.leader.clone();
            let lifecycle = self.lifecycle.clone();
            let slot_clock = Arc::clone(&self.slot_clock);
            let mut rx = event_rx;
            handles.push(tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    if let StreamEvent::Slot(slot) = &event {
                        slot_clock.fetch_max(slot.slot, Ordering::Relaxed);
                    }
                    leader.ingest(&event);
                    lifecycle.ingest(&event);
                }
                info!("stream event channel closed; fan-out stopping");
            }));
        }

        // Leader schedule + Jito-set refresh loops.
        self.leader.spawn_refresh();

        // Background blockhash refresher — keeps a fresh confirmed blockhash in
        // the cache so the submission hot path makes NO blockhash RPC call.
        handles.push(submitter::spawn_blockhash_refresher(
            self.blockhash_cache.clone(),
            Arc::clone(&self.reconcile_rpc),
            Arc::clone(&self.slot_clock),
            BLOCKHASH_REFRESH,
        ));

        // Background tip-account warmer — pre-fetches the tip accounts at startup
        // (so the FIRST submission reads them from cache, no network) and keeps
        // them fresh. This also warms the keep-alive connection to the block
        // engine that send_bundle reuses, so the TLS handshake isn't paid in the
        // hot path.
        {
            let submitter = Arc::clone(&self.submitter);
            handles.push(tokio::spawn(async move {
                loop {
                    match submitter.warm_tip_cache().await {
                        Ok(()) => debug!("tip-account cache + block-engine connection warmed"),
                        Err(err) => warn!(
                            error = %runtime::redact_url(&err.to_string()),
                            "tip-account warm failed; will retry"
                        ),
                    }
                    tokio::time::sleep(TIP_REFRESH).await;
                }
            }));
        }

        // Tips feed.
        {
            let tips = self.tips.clone();
            handles.push(tokio::spawn(async move {
                if let Err(err) = tips.run().await {
                    warn!(error = %err, "tips tracker exited");
                }
            }));
        }

        // Reconnect → reconcile.
        {
            let app = Arc::clone(self);
            let mut rx = reconnect_rx;
            handles.push(tokio::spawn(async move {
                while let Some(attempt) = rx.recv().await {
                    info!(attempt, "stream reconnected; running lifecycle reconcile");
                    match app.lifecycle.reconcile(&*app.reconcile_rpc).await {
                        Ok(report) => info!(
                            checked = report.checked,
                            applied = report.transitions.len(),
                            "reconcile complete"
                        ),
                        Err(err) => warn!(
                            error = %runtime::redact_url(&err.to_string()),
                            "reconcile failed"
                        ),
                    }
                }
            }));
        }

        // Timeout sweeper (~every 10 slots).
        {
            let app = Arc::clone(self);
            handles.push(tokio::spawn(async move {
                let mut ticker =
                    tokio::time::interval(Duration::from_millis(SLOT_MILLIS * 10));
                loop {
                    ticker.tick().await;
                    match app.lifecycle.check_timeouts().await {
                        Ok(ids) if !ids.is_empty() => {
                            for id in ids {
                                if let Err(err) = app.handle_timed_out(id).await {
                                    warn!(bundle_db_id = id, error = %err, "timeout handling failed");
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(err) => warn!(error = %err, "check_timeouts failed"),
                    }
                }
            }));
        }

        handles
    }

    /// Block until the system is warm: the slot clock is ticking and the leader
    /// schedule + Jito set are loaded (i.e. `current_window` succeeds).
    pub async fn await_ready(&self, timeout: Duration) -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        info!("warming up: waiting for slot clock + leader schedule + Jito set...");
        loop {
            match self.leader.current_window().await {
                Ok(window) => {
                    info!(
                        slot = window.current_slot,
                        next_jito_leader_slot = window.next_jito_leader_slot,
                        "system ready"
                    );
                    return Ok(());
                }
                Err(err) => {
                    if std::time::Instant::now() >= deadline {
                        anyhow::bail!("system not ready within timeout: {err}");
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Highest bundle row id currently in the DB (0 if empty). Used to scope the
    /// drain to bundles created by *this* run.
    pub async fn max_bundle_id(&self) -> anyhow::Result<i64> {
        let id: Option<i64> = sqlx::query_scalar("SELECT MAX(id) FROM bundle_submissions")
            .fetch_one(&self.pool)
            .await?;
        Ok(id.unwrap_or(0))
    }

    /// Status counts for bundle rows created after `baseline_id` (this run).
    async fn run_status_counts(&self, baseline_id: i64) -> anyhow::Result<StatusCounts> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT status, COUNT(*) FROM bundle_submissions WHERE id > ? GROUP BY status",
        )
        .bind(baseline_id)
        .fetch_all(&self.pool)
        .await?;
        let mut c = StatusCounts::default();
        for (status, n) in rows {
            match status.as_str() {
                "Submitted" => c.submitted = n,
                "Processed" => c.processed = n,
                "Confirmed" => c.confirmed = n,
                "Finalized" => c.finalized = n,
                "Failed" => c.failed = n,
                _ => c.other += n,
            }
        }
        Ok(c)
    }

    /// Drain phase: poll until every bundle created by this run (id >
    /// `baseline_id`) is terminal (Finalized or Failed), or `max_wait` elapses.
    /// Logs the per-state breakdown roughly every 10s so the operator sees
    /// movement. The timeout sweeper (spawned in `spawn_background`) keeps running
    /// throughout — this method only polls, it doesn't block the runtime — so
    /// accepted-but-never-landed bundles are swept to NeverLanded/Failed here
    /// rather than hanging at Submitted.
    pub async fn drain_to_terminal(&self, baseline_id: i64, max_wait: Duration) {
        let deadline = std::time::Instant::now() + max_wait;
        let mut last_log: Option<std::time::Instant> = None;
        loop {
            let counts = match self.run_status_counts(baseline_id).await {
                Ok(c) => c,
                Err(err) => {
                    warn!(error = %err, "drain: status query failed; retrying");
                    StatusCounts::default()
                }
            };

            if last_log.is_none_or(|t| t.elapsed() >= Duration::from_secs(10)) {
                info!(
                    submitted = counts.submitted,
                    processed = counts.processed,
                    confirmed = counts.confirmed,
                    finalized = counts.finalized,
                    failed = counts.failed,
                    "drain progress (waiting for terminal states)"
                );
                last_log = Some(std::time::Instant::now());
            }

            if counts.non_terminal() == 0 {
                info!(
                    finalized = counts.finalized,
                    failed = counts.failed,
                    "all bundles from this run reached a terminal state"
                );
                return;
            }
            if std::time::Instant::now() >= deadline {
                warn!(
                    non_terminal = counts.non_terminal(),
                    submitted = counts.submitted,
                    processed = counts.processed,
                    confirmed = counts.confirmed,
                    "drain max wait reached; exiting with bundles still in flight"
                );
                return;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// The one submission pipeline used by all modes.
    pub async fn submit_one(&self, mut spec: BundleSpec) -> anyhow::Result<()> {
        // 1. Wait for a Jito leader window.
        let window = match self.leader.wait_for_window().await {
            Ok(w) => w,
            Err(err) => {
                warn!(error = %err, "leader window unavailable; skipping this submission");
                return Ok(());
            }
        };
        info!(
            slot = window.current_slot,
            slots_until = window.slots_until,
            next_jito_leader_slot = window.next_jito_leader_slot,
            leader = %window.leader_identity.as_deref().unwrap_or("?"),
            is_bam = window.is_bam,
            "Jito leader window open"
        );

        // Auction-aware optimization: a BAM leader scores on (tips + priority_fees)
        // / CU, so when enabled we add a priority fee for BAM targets to stay
        // competitive. Non-BAM leaders (Block Engine, tips/CU) are unchanged.
        let apply_bam_fee = window.is_bam && self.config.bam_priority_fee_enabled;
        spec.priority_fee_microlamports = if apply_bam_fee {
            self.config.bam_priority_fee_microlamports
        } else {
            0
        };
        spec.priority_fee_cu_limit = if apply_bam_fee {
            self.config.bam_priority_fee_cu_limit
        } else {
            0
        };
        if spec.priority_fee_microlamports > 0 {
            // Estimated fee = ceil(price × limit / 1e6) when a CU limit is set.
            let est_fee_lamports = if spec.priority_fee_cu_limit > 0 {
                (spec.priority_fee_microlamports as u128 * spec.priority_fee_cu_limit as u128)
                    .div_ceil(1_000_000) as u64
            } else {
                0
            };
            info!(
                priority_fee_microlamports = spec.priority_fee_microlamports,
                cu_limit = spec.priority_fee_cu_limit,
                est_priority_fee_lamports = est_fee_lamports,
                "BAM leader — adding priority fee to compete in (tips + prio-fees)/CU auction"
            );
        }

        // TIMING telemetry: mark the moment the window opened so we can measure
        // how many ms / slots elapse before the bundle is actually sent. If this
        // routinely spans > ~400ms (one slot), we're arriving after the leader.
        let t_window_open = std::time::Instant::now();
        let slot_at_window = self.slot_clock.load(Ordering::Relaxed);

        // 2. Tip policy for NORMAL submissions (the agent owns tips on retries).
        let policy = normal_tip_policy(
            self.tips.latest(),
            self.tips.is_stale(),
            self.config.tip_percentile,
            self.config.max_tip_lamports,
        );
        if policy.source == "live" {
            info!(
                percentile = self.config.tip_percentile.as_str(),
                tip = policy.tip,
                p50 = ?policy.p50,
                p75 = ?policy.p75,
                "normal tip policy"
            );
        } else {
            warn!(
                source = policy.source,
                tip = policy.tip,
                "tip data not live — using fallback tip ({} lamports)",
                policy.tip
            );
        }
        // Set the normal tip. (A SubFloorTip fault, if any, overrides this inside
        // the submitter; the agent's SetTip overrides it on retries.)
        spec.tip_lamports = policy.tip;
        let current_slot = window.current_slot;

        // 3. Submit (timed end-to-end).
        let result = self.submitter.submit_bundle(spec.clone(), current_slot).await;
        let slot_at_send = self.slot_clock.load(Ordering::Relaxed);
        info!(
            slots_until_at_window = window.slots_until,
            target_leader_slot = window.next_jito_leader_slot,
            slot_at_window,
            slot_at_send,
            slots_drifted = slot_at_send.saturating_sub(slot_at_window),
            elapsed_ms = t_window_open.elapsed().as_millis(),
            "TIMING(orchestrator): elapsed between leader-window-open and submit_bundle returning \
             (build + sign + fetch-tip + network send)"
        );
        match result {
            Ok(record) => {
                // 4. Track the accepted bundle.
                let row = self
                    .lifecycle
                    .record_submission(&record, policy.p50, policy.p75)
                    .await?;
                info!(
                    row,
                    bundle_id = %record.bundle_id,
                    tip = record.tip_lamports,
                    slot = current_slot,
                    memo_sig = %record.memo_signature,
                    "bundle submitted; tracking lifecycle"
                );
                // Authoritative second signal: ask Jito directly whether this
                // accepted bundle enters the auction (Pending) and what becomes of
                // it (Landed / Failed / Invalid). Runs alongside the on-chain
                // lifecycle tracking so we can compare the two.
                spawn_bundle_status_poll(
                    Arc::clone(&self.submitter),
                    self.lifecycle.clone(),
                    row,
                    record.bundle_id.clone(),
                );
                Ok(())
            }
            Err(err) => {
                // 5. Synchronous rejection → record + classify + agent retry loop.
                let raw = err.to_string();
                warn!(
                    error = %runtime::redact_url(&raw),
                    "===== BLOCK ENGINE REJECTION (raw) ===== {raw}"
                );
                let id = self
                    .record_synthetic_rejection(&spec, current_slot, &raw, policy.p50, policy.p75)
                    .await?;
                let evidence = failure::Evidence::SubmitRejection {
                    raw_error: raw,
                };
                let classification = failure::classify(&evidence);
                info!(
                    bundle_db_id = id,
                    kind = ?classification.kind,
                    confidence = ?classification.confidence,
                    "classified rejection; entering agent retry loop"
                );

                // Real retries should not carry the fault that caused the failure.
                clear_fault(&mut spec);
                self.run_retry(id, classification, evidence, spec, current_slot)
                    .await
            }
        }
    }

    /// Drive the agent retry loop with this app's live dependencies.
    async fn run_retry(
        &self,
        bundle_db_id: i64,
        classification: failure::Classification,
        evidence: failure::Evidence,
        spec: BundleSpec,
        blockhash_fetched_at_slot: u64,
    ) -> anyhow::Result<()> {
        let tips = &self.tips;
        let tip_provider = || {
            let snap = tips.latest();
            TipCtx {
                p50: snap.as_ref().map(|s| s.p50_lamports),
                p75: snap.as_ref().map(|s| s.p75_lamports),
                trend: tips.trend(TREND_WINDOW),
                age_secs: tips.freshness().map(|d| d.as_secs()),
            }
        };
        agent_retry_loop(
            &*self.submitter,
            &self.llm,
            &self.baseline,
            &self.lifecycle,
            &self.agent_log,
            &self.pool,
            &self.slot_clock,
            &LoopConfig {
                max_attempts: self.config.max_attempts,
                max_tip: self.config.max_tip_lamports,
                model: self.config.agent_model.clone(),
            },
            bundle_db_id,
            classification,
            evidence,
            spec,
            blockhash_fetched_at_slot,
            tip_provider,
        )
        .await
    }

    /// Insert a lifecycle row for a synchronously-rejected bundle (which never
    /// produced a real bundle id / signature), then mark it Failed.
    async fn record_synthetic_rejection(
        &self,
        spec: &BundleSpec,
        current_slot: u64,
        raw_error: &str,
        p50: Option<u64>,
        p75: Option<u64>,
    ) -> anyhow::Result<i64> {
        let uid = REJECTION_SEQ.fetch_add(1, Ordering::Relaxed);
        let marker = format!("rejected-{current_slot}-{uid}");
        let record = BundleRecord {
            bundle_id: marker.clone(),
            tip_lamports: spec.tip_lamports,
            tip_account: String::new(),
            memo_signature: marker.clone(),
            tip_signature: String::new(),
            blockhash: String::new(),
            blockhash_fetched_at_slot: current_slot,
            submitted_at: SystemTime::now(),
            fault_injected: None,
        };
        let id = self.lifecycle.record_submission(&record, p50, p75).await?;
        self.lifecycle.record_rejection(id, raw_error).await?;
        Ok(id)
    }

    /// Handle a bundle the sweeper found timed out (NeverLanded): rebuild the
    /// evidence/context from its row and run the agent loop.
    async fn handle_timed_out(&self, id: i64) -> anyhow::Result<()> {
        let row = self.load_bundle_row(id).await?;
        let last = self
            .slot_clock
            .load(Ordering::Relaxed)
            .max(row.blockhash_fetched_at_slot);
        let evidence = failure::Evidence::NeverLanded {
            submitted_slot: row.submitted_slot,
            blockhash_fetched_at_slot: row.blockhash_fetched_at_slot,
            last_observed_slot: last,
            tip_lamports: row.tip_lamports,
            tip_p50_at_submit: row.tip_p50,
            tip_p75_at_submit: row.tip_p75,
            jito_inflight: failure::JitoInflight::from_status_str(
                row.jito_inflight_status.as_deref(),
            ),
        };
        let classification = failure::classify(&evidence);
        warn!(
            bundle_db_id = id,
            kind = ?classification.kind,
            "bundle timed out (never landed); entering agent retry loop"
        );
        let spec = base_spec(row.tip_lamports, format!("retry-of-{id}"));
        self.run_retry(
            id,
            classification,
            evidence,
            spec,
            row.blockhash_fetched_at_slot,
        )
        .await
    }

    async fn load_bundle_row(&self, id: i64) -> anyhow::Result<BundleRow> {
        let (tip, p50, p75, bh_slot, sub_slot, jito): (
            i64,
            Option<i64>,
            Option<i64>,
            i64,
            i64,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT tip_lamports, tip_p50_at_submit, tip_p75_at_submit, \
                 blockhash_fetched_at_slot, submitted_slot, jito_inflight_status \
                 FROM bundle_submissions WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(BundleRow {
            tip_lamports: tip as u64,
            tip_p50: p50.map(|v| v as u64),
            tip_p75: p75.map(|v| v as u64),
            blockhash_fetched_at_slot: bh_slot as u64,
            submitted_slot: sub_slot as u64,
            jito_inflight_status: jito,
        })
    }
}

struct BundleRow {
    tip_lamports: u64,
    tip_p50: Option<u64>,
    tip_p75: Option<u64>,
    blockhash_fetched_at_slot: u64,
    submitted_slot: u64,
    jito_inflight_status: Option<String>,
}

/// Clear any injected fault on a spec (so agent retries are real submissions).
#[allow(unused_variables, unused_mut)]
fn clear_fault(spec: &mut BundleSpec) {
    #[cfg(feature = "fault-injection")]
    {
        spec.fault = None;
    }
}

// ---------------------------------------------------------------------------
// Pool / status / export helpers
// ---------------------------------------------------------------------------

/// Open (creating if needed) the SQLite pool at `db_path`.
pub async fn open_pool(db_path: &str) -> anyhow::Result<SqlitePool> {
    let url = format!("sqlite://{db_path}?mode=rwc");
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .with_context(|| format!("opening SQLite database at {db_path}"))?;
    Ok(pool)
}

/// A `bundle_submissions` row, as selected for the status table.
type StatusRow = (i64, String, String, Option<i64>, i64, Option<String>);

/// Print pending/terminal counts and the last 10 bundles.
#[allow(clippy::print_literal)] // aligned literal header columns read clearly
pub async fn print_status(pool: &SqlitePool) -> anyhow::Result<()> {
    let counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, COUNT(*) FROM bundle_submissions GROUP BY status")
            .fetch_all(pool)
            .await?;
    println!("Bundle status counts:");
    if counts.is_empty() {
        println!("  (none)");
    }
    for (status, n) in &counts {
        println!("  {status:<10} {n}");
    }

    let recent: Vec<StatusRow> = sqlx::query_as(
        "SELECT id, bundle_id, status, landed_slot, tip_lamports, failure_kind \
         FROM bundle_submissions ORDER BY id DESC LIMIT 10",
    )
    .fetch_all(pool)
    .await?;

    println!("\nLast {} bundles:", recent.len());
    println!(
        "  {:>4}  {:<24}  {:<10}  {:>12}  {:>10}  {}",
        "id", "bundle_id", "status", "landed_slot", "tip", "failure"
    );
    for (id, bundle_id, status, landed, tip, failure) in recent {
        println!(
            "  {:>4}  {:<24}  {:<10}  {:>12}  {:>10}  {}",
            id,
            truncate(&bundle_id, 24),
            status,
            landed.map(|v| v.to_string()).unwrap_or_default(),
            tip,
            failure.unwrap_or_default(),
        );
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

// ---------------------------------------------------------------------------
// Tests (offline)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use agent::Decision;
    use std::sync::Mutex;

    // --- drain terminal-detection ---

    #[test]
    fn status_counts_non_terminal_excludes_finalized_and_failed() {
        // Submitted/Processed/Confirmed are in-flight; Finalized/Failed are terminal.
        let counts = StatusCounts {
            submitted: 2,
            processed: 1,
            confirmed: 1,
            finalized: 3,
            failed: 4,
            other: 0,
        };
        assert_eq!(counts.non_terminal(), 4); // 2 + 1 + 1

        // All terminal -> drain completes.
        let done = StatusCounts {
            finalized: 5,
            failed: 5,
            ..Default::default()
        };
        assert_eq!(done.non_terminal(), 0);
    }

    // --- tip policy ---

    fn snap(p50: u64, p75: u64) -> TipSnapshot {
        snap_full(p50, p75, 0)
    }

    fn snap_full(p50: u64, p75: u64, p95: u64) -> TipSnapshot {
        TipSnapshot {
            taken_at: std::time::Instant::now(),
            p25_lamports: 0,
            p50_lamports: p50,
            p75_lamports: p75,
            p95_lamports: p95,
            p99_lamports: 0,
            ema_p50_lamports: p50,
        }
    }

    #[test]
    fn tip_policy_targets_configured_percentile() {
        // Default p75 targets the p75 value.
        let p = normal_tip_policy(Some(snap(2_000, 9_000)), false, TipPercentile::P75, 100_000);
        assert_eq!(p.tip, 9_000);
        assert_eq!(p.source, "live");
        assert_eq!(p.p50, Some(2_000));
        assert_eq!(p.p75, Some(9_000));

        // p50 and p95 are selectable.
        let s = snap_full(2_000, 9_000, 25_000);
        assert_eq!(normal_tip_policy(Some(s), false, TipPercentile::P50, 100_000).tip, 2_000);
        assert_eq!(normal_tip_policy(Some(s), false, TipPercentile::P95, 100_000).tip, 25_000);
    }

    #[test]
    fn tip_policy_floor_and_cap() {
        // Below the 1_000 floor (p75 of 800).
        assert_eq!(
            normal_tip_policy(Some(snap(500, 800)), false, TipPercentile::P75, 100_000).tip,
            1_000
        );
        // Above the cap.
        assert_eq!(
            normal_tip_policy(Some(snap(250_000, 300_000)), false, TipPercentile::P75, 100_000).tip,
            100_000
        );
    }

    #[test]
    fn tip_policy_stale_or_absent_falls_back() {
        // No data -> 10_000 fallback.
        let none = normal_tip_policy(None, false, TipPercentile::P75, 100_000);
        assert_eq!(none.tip, STALE_TIP_FALLBACK);
        assert_eq!(none.source, "no-data");

        // Stale-but-present -> 10_000 fallback (don't trust a stale percentile),
        // but keep p50/p75 for context.
        let stale = normal_tip_policy(Some(snap(3_000, 5_000)), true, TipPercentile::P75, 100_000);
        assert_eq!(stale.tip, STALE_TIP_FALLBACK);
        assert_eq!(stale.source, "stale");
        assert_eq!(stale.p75, Some(5_000));

        // Fallback is still capped by max_tip.
        assert_eq!(normal_tip_policy(None, false, TipPercentile::P75, 4_000).tip, 4_000);
    }

    #[test]
    fn tip_percentile_parse() {
        assert_eq!(TipPercentile::parse("p50"), Some(TipPercentile::P50));
        assert_eq!(TipPercentile::parse("P75"), Some(TipPercentile::P75));
        assert_eq!(TipPercentile::parse(" p95 "), Some(TipPercentile::P95));
        assert_eq!(TipPercentile::parse("p99"), None);
        assert_eq!(TipPercentile::P75.as_str(), "p75");
    }

    // --- plan_actions (execution ordering) ---

    #[test]
    fn plan_actions_orders_modifiers_then_terminal() {
        let plan = plan_actions(&[
            Action::RefreshBlockhash,
            Action::SetTip(5_000),
            Action::Resubmit,
        ]);
        assert!(plan.refresh);
        assert_eq!(plan.set_tip, Some(5_000));
        assert_eq!(plan.terminal, Terminal::Resubmit);
    }

    #[test]
    fn plan_actions_terminal_stops_processing() {
        // Anything after the first terminal is ignored.
        let plan = plan_actions(&[Action::Resubmit, Action::SetTip(9_000)]);
        assert_eq!(plan.set_tip, None);
        assert_eq!(plan.terminal, Terminal::Resubmit);

        let plan2 = plan_actions(&[Action::Hold { slots: 3 }, Action::Abandon]);
        assert_eq!(plan2.hold_slots, 3);
        assert_eq!(plan2.terminal, Terminal::Abandon);

        // Later SetTip wins among modifiers before the terminal.
        let plan3 = plan_actions(&[Action::SetTip(1), Action::SetTip(7), Action::Resubmit]);
        assert_eq!(plan3.set_tip, Some(7));
    }

    // --- retry loop: attempt cap + SetTip override ---

    struct MockSubmitter {
        calls: Arc<Mutex<Vec<(u64, u64)>>>, // (tip, slot) per call
        ok_after: Option<usize>,            // return Ok once call count reaches this
    }

    impl Submit for MockSubmitter {
        async fn submit_bundle(
            &self,
            spec: BundleSpec,
            current_slot: u64,
        ) -> Result<BundleRecord, SubmitError> {
            let mut calls = self.calls.lock().unwrap();
            calls.push((spec.tip_lamports, current_slot));
            let n = calls.len();
            drop(calls);
            match self.ok_after {
                Some(k) if n >= k => Ok(BundleRecord {
                    bundle_id: format!("ok-{n}"),
                    tip_lamports: spec.tip_lamports,
                    tip_account: String::new(),
                    memo_signature: format!("ok-sig-{n}"),
                    tip_signature: String::new(),
                    blockhash: String::new(),
                    blockhash_fetched_at_slot: current_slot,
                    submitted_at: SystemTime::now(),
                    fault_injected: None,
                }),
                _ => Err(SubmitError::Rejected {
                    reason: "mock rejection".to_string(),
                }),
            }
        }
    }

    struct FixedAgent {
        decision: Decision,
    }

    impl DecisionAgent for FixedAgent {
        async fn decide(&self, _ctx: &DecisionContext) -> anyhow::Result<Decision> {
            Ok(self.decision.clone())
        }
    }

    async fn mem_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    async fn loop_harness(
        submitter: &MockSubmitter,
        agent_decision: Decision,
        max_attempts: u32,
    ) -> SqlitePool {
        let pool = mem_pool().await;
        let lifecycle = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        let agent_log = agent::AgentLog::new(pool.clone()).await.unwrap();
        let baseline = BaselineAgent;
        let llm = FixedAgent {
            decision: agent_decision,
        };
        let slot_clock = Arc::new(AtomicU64::new(500));
        let evidence = failure::Evidence::SubmitRejection {
            raw_error: "mock".to_string(),
        };
        let classification = failure::classify(&evidence);

        agent_retry_loop(
            submitter,
            &llm,
            &baseline,
            &lifecycle,
            &agent_log,
            &pool,
            &slot_clock,
            &LoopConfig {
                max_attempts,
                max_tip: 1_000_000,
                model: "test-model".to_string(),
            },
            1,
            classification,
            evidence,
            base_spec(10_000, "m".to_string()),
            500,
            TipCtx::default,
        )
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn retry_loop_respects_attempt_cap() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let submitter = MockSubmitter {
            calls: Arc::clone(&calls),
            ok_after: None, // always reject
        };
        let decision = Decision {
            actions: vec![Action::Resubmit],
            rationale: "retry".to_string(),
        };
        let pool = loop_harness(&submitter, decision, 4).await;

        // max_attempts=4, initial failure is attempt 1 -> resubmits for attempts
        // 2,3,4 = 3 submissions, then the cap stops it.
        assert_eq!(calls.lock().unwrap().len(), 3);

        // A decision row was recorded for each of attempts 2,3,4 (executed=1).
        let executed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_decisions WHERE executed = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(executed, 3);
    }

    #[tokio::test]
    async fn retry_loop_applies_set_tip_in_order_before_resubmit() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let submitter = MockSubmitter {
            calls: Arc::clone(&calls),
            ok_after: Some(1), // first resubmit succeeds, ending the loop
        };
        let decision = Decision {
            actions: vec![Action::SetTip(777), Action::Resubmit],
            rationale: "raise tip then resubmit".to_string(),
        };
        loop_harness(&submitter, decision, 4).await;

        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        // The resubmission used the agent-set tip (ordering: SetTip before Resubmit).
        assert_eq!(recorded[0].0, 777);
    }

    #[tokio::test]
    async fn retry_loop_abandon_stops_immediately() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let submitter = MockSubmitter {
            calls: Arc::clone(&calls),
            ok_after: None,
        };
        let decision = Decision {
            actions: vec![Action::Abandon],
            rationale: "give up".to_string(),
        };
        loop_harness(&submitter, decision, 4).await;
        // Abandon means no resubmission at all.
        assert_eq!(calls.lock().unwrap().len(), 0);
    }
}
