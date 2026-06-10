//! Env-based configuration layer.
//!
//! Loads from the process environment (optionally seeded by a `.env` file).
//! Secrets live only in the environment / `.env` — never commit them.

use anyhow::Context;

/// All runtime configuration for the stack.
// Several fields are only consumed once the (stubbed) control loop wires up the
// subsystems that need them; allow until then.
#[allow(dead_code)]
#[derive(Debug, Clone)]
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
            wallet_keypair_path: required("WALLET_KEYPAIR_PATH")?,
            anthropic_api_key: required("ANTHROPIC_API_KEY")?,
        })
    }
}

/// Read a required env var, erroring with a clear message if it's unset.
fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var: {key}"))
}
