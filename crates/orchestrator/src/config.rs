//! Env-based configuration layer.
//!
//! Loads from the process environment (optionally seeded by a `.env` file).
//! Secrets live only in the environment / `.env` — never commit them.

use std::time::Duration;

use anyhow::Context;

/// All runtime configuration for the stack.
//
// NOTE: `Debug` is implemented by hand below so secrets are masked and URLs
// redacted — never derive `Debug` here, or `{:?}` will leak credentials.
#[derive(Clone)]
pub struct Config {
    /// Solana JSON-RPC URL.
    pub rpc_url: String,
    /// Yellowstone gRPC endpoint.
    pub yellowstone_endpoint: String,
    /// Yellowstone `x-token` auth header (optional, provider-dependent).
    pub yellowstone_x_token: Option<String>,
    /// Jito block-engine region base URL.
    pub jito_block_engine_url: String,
    /// Path to the wallet keypair JSON file.
    pub wallet_keypair_path: String,
    /// Anthropic API key for the reasoning agent.
    pub anthropic_api_key: String,
    /// SQLite database path.
    pub db_path: String,
    /// Hard ceiling for any tip (lamports).
    pub max_tip_lamports: u64,
    /// Lamports for the wallet -> wallet self-transfer in the payload tx (real
    /// economic content so the auction schedules the bundle). Default 1000.
    pub self_transfer_lamports: u64,
    /// Requests/second budget for all Jito block-engine calls. Anonymous tier = 1;
    /// an auth uuid raises it to 5. Default 1.
    pub jito_rps: u32,
    /// Optional Jito auth uuid (`x-jito-auth` header). Raises the rate limit to
    /// 5 RPS. `None` = anonymous.
    pub jito_auth_uuid: Option<String>,
    /// Auction-aware optimization (off by default): when the target leader runs
    /// BAM, add a priority fee so we compete in its `(tips + priority_fees) / CU`
    /// auction. Toggled by `BAM_PRIORITY_FEE_ENABLED`.
    pub bam_priority_fee_enabled: bool,
    /// Priority fee (micro-lamports/CU) added for BAM leaders when enabled.
    /// `BAM_PRIORITY_FEE_MICROLAMPORTS`.
    pub bam_priority_fee_microlamports: u64,
    /// Hard cap on total submission attempts per bundle (including the first).
    pub max_attempts: u32,
    /// Anthropic model id.
    pub agent_model: String,
    /// Per-LLM-request timeout.
    pub agent_timeout: Duration,
    /// Max time `run` waits for all submitted bundles to reach a terminal state
    /// before exiting. Must exceed the ~150-slot (~60s) never-landed timeout.
    pub drain_timeout: Duration,
    /// Which landed-tip percentile the normal-submission tip policy targets.
    pub tip_percentile: crate::app::TipPercentile,
}

impl std::fmt::Debug for Config {
    /// Credential-safe: URLs are redacted and tokens/keys masked (first 4 chars
    /// + `…`), so logging the config can never leak secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("rpc_url", &runtime::redact_url(&self.rpc_url))
            .field(
                "yellowstone_endpoint",
                &runtime::redact_url(&self.yellowstone_endpoint),
            )
            .field(
                "yellowstone_x_token",
                &self.yellowstone_x_token.as_deref().map(runtime::mask_secret),
            )
            .field(
                "jito_block_engine_url",
                &runtime::redact_url(&self.jito_block_engine_url),
            )
            .field("wallet_keypair_path", &self.wallet_keypair_path)
            .field(
                "anthropic_api_key",
                &runtime::mask_secret(&self.anthropic_api_key),
            )
            .field("db_path", &self.db_path)
            .field("max_tip_lamports", &self.max_tip_lamports)
            .field("self_transfer_lamports", &self.self_transfer_lamports)
            .field("jito_rps", &self.jito_rps)
            .field(
                "jito_auth_uuid",
                &self.jito_auth_uuid.as_deref().map(runtime::mask_secret),
            )
            .field("bam_priority_fee_enabled", &self.bam_priority_fee_enabled)
            .field(
                "bam_priority_fee_microlamports",
                &self.bam_priority_fee_microlamports,
            )
            .field("max_attempts", &self.max_attempts)
            .field("agent_model", &self.agent_model)
            .field("tip_percentile", &self.tip_percentile.as_str())
            .finish()
    }
}

impl Config {
    /// Load `.env` (if present) and read configuration from the environment.
    pub fn from_env() -> anyhow::Result<Self> {
        // Best-effort: missing .env is fine, real env vars still apply.
        let _ = dotenvy::dotenv();

        Ok(Self {
            rpc_url: required("RPC_URL")?,
            yellowstone_endpoint: required("YELLOWSTONE_ENDPOINT")?,
            yellowstone_x_token: std::env::var("YELLOWSTONE_X_TOKEN").ok(),
            jito_block_engine_url: required("JITO_BLOCK_ENGINE_URL")?,
            // Accept WALLET_PATH (preferred) or the legacy WALLET_KEYPAIR_PATH.
            wallet_keypair_path: first_set(&["WALLET_PATH", "WALLET_KEYPAIR_PATH"])
                .context("missing required env var: WALLET_PATH (or WALLET_KEYPAIR_PATH)")?,
            anthropic_api_key: required("ANTHROPIC_API_KEY")?,
            db_path: optional("DB_PATH", "./smart_tx.db"),
            max_tip_lamports: parse_or("MAX_TIP_LAMPORTS", 100_000)?,
            self_transfer_lamports: parse_or("SELF_TRANSFER_LAMPORTS", 1_000)?,
            jito_rps: parse_or("JITO_RPS", 1)?,
            jito_auth_uuid: std::env::var("JITO_AUTH_UUID").ok().filter(|s| !s.is_empty()),
            bam_priority_fee_enabled: bool_flag("BAM_PRIORITY_FEE_ENABLED"),
            bam_priority_fee_microlamports: parse_or("BAM_PRIORITY_FEE_MICROLAMPORTS", 0)?,
            max_attempts: parse_or("MAX_ATTEMPTS", 4)?,
            agent_model: optional("AGENT_MODEL", agent::DEFAULT_MODEL),
            agent_timeout: Duration::from_secs(10),
            drain_timeout: Duration::from_secs(parse_or("RUN_DRAIN_SECS", 120)?),
            tip_percentile: {
                let raw = optional("TIP_PERCENTILE", "p75");
                crate::app::TipPercentile::parse(&raw).with_context(|| {
                    format!("invalid TIP_PERCENTILE {raw:?} (expected p50|p75|p95)")
                })?
            },
        })
    }
}

/// Read a required env var, erroring with a clear message if it's unset.
fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var: {key}"))
}

/// First of `keys` that is set in the environment.
fn first_set(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| std::env::var(k).ok())
}

/// Read an env var, falling back to `default` if unset.
fn optional(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// A boolean env flag: true for `1`/`true`/`yes` (case-insensitive); else false.
fn bool_flag(key: &str) -> bool {
    std::env::var(key)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// Parse a numeric env var, falling back to `default` if unset (error if set but
/// unparseable).
fn parse_or<T>(key: &str, default: T) -> anyhow::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(key) {
        Ok(raw) => raw
            .parse::<T>()
            .map_err(|e| anyhow::anyhow!("env var {key} is not a valid number: {e}")),
        Err(_) => Ok(default),
    }
}
