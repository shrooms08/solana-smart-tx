//! Orchestrator: the control loop that wires every crate together.
//!
//! Responsibilities (all TODO — this is a skeleton):
//!   * ingest slots + tx statuses (`stream`)
//!   * detect Jito leader windows (`leader`)
//!   * track tip floors (`tips`)
//!   * build + submit bundles (`submitter`)
//!   * persist lifecycle + classify failures (`lifecycle`, `failure`)
//!   * ask the decision layer what to do next (`agent`)

mod config;

use anyhow::Context;
use config::Config;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Config::from_env().context("failed to load configuration")?;
    info!(
        rpc_url = %config.rpc_url,
        yellowstone = %config.yellowstone_endpoint,
        jito = %config.jito_block_engine_url,
        "orchestrator starting"
    );

    #[cfg(feature = "fault-injection")]
    tracing::warn!("fault-injection feature is ENABLED — do not use in production");

    // TODO: construct each subsystem from `config` and run the control loop:
    //   let stream = stream::StreamClient::new(...);
    //   let store  = lifecycle::LifecycleStore::connect(...).await?;
    //   let agent  = agent::BaselineAgent::default();
    //   ... select! over events, drive submissions, persist, decide ...
    run(config).await
}

/// Initialize `tracing` with an `RUST_LOG`-driven env filter.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();
}

/// The main control loop.
///
/// TODO: implement. For now it just confirms the wiring compiles.
async fn run(_config: Config) -> anyhow::Result<()> {
    info!("control loop not implemented yet — exiting cleanly");
    Ok(())
}
