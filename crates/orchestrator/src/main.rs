//! Orchestrator: the running system that wires every crate together.
//!
//! Subcommands:
//!   * `run`    — submit N normal bundles (the judged happy-path log)
//!   * `fault`  — inject a single fault (requires `--features fault-injection`)
//!   * `export` — write the lifecycle log to `./exports/`
//!   * `status` — print pending/terminal counts + the last 10 bundles

mod app;
mod config;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Parser, Subcommand};
use config::Config;
use lifecycle::{LifecycleConfig, LifecycleTracker};
use tracing::{info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use app::App;

#[derive(Parser)]
#[command(name = "orchestrator", about = "Smart Transaction Stack control loop")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Submit N normal bundles, spaced by `--interval-slots`.
    Run {
        #[arg(long, default_value_t = 10)]
        count: u32,
        #[arg(long, default_value_t = 150)]
        interval_slots: u64,
    },
    /// Inject a single fault (requires building with `--features fault-injection`).
    Fault {
        #[command(subcommand)]
        kind: FaultKind,
    },
    /// Export the lifecycle log to `./exports/`.
    Export,
    /// Print pending/terminal counts and the last 10 bundles.
    Status,
}

#[derive(Subcommand, Clone, Copy)]
enum FaultKind {
    /// Hold until the blockhash is well past its validity window, then submit.
    StaleBlockhash,
    /// Submit with a sub-floor (500 lamport) tip; expect a Block Engine rejection.
    SubFloorTip,
}

/// Live run modes (built only when the relevant feature is enabled).
enum LiveMode {
    Run { count: u32, interval_slots: u64 },
    #[cfg(feature = "fault-injection")]
    Fault(FaultKind),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the rustls CryptoProvider before any TLS client connects.
    runtime::init_crypto();
    init_tracing();

    let cli = Cli::parse();
    let config = Config::from_env().context("failed to load configuration")?;
    info!(config = ?config, "orchestrator starting");

    #[cfg(feature = "fault-injection")]
    warn!("fault-injection feature is ENABLED — do not use in production");

    match cli.command {
        Command::Run {
            count,
            interval_slots,
        } => {
            run_live(
                config,
                LiveMode::Run {
                    count,
                    interval_slots,
                },
            )
            .await
        }
        Command::Fault { kind } => run_fault(config, kind).await,
        Command::Export => export(config).await,
        Command::Status => status(config).await,
    }
}

/// Initialize `tracing` with an `RUST_LOG`-driven env filter.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();
}

// ---------------------------------------------------------------------------
// Live run (run / fault)
// ---------------------------------------------------------------------------

async fn run_live(config: Config, mode: LiveMode) -> anyhow::Result<()> {
    let drain_timeout = config.drain_timeout;
    let app = Arc::new(App::build(config).await?);
    // The background tasks (stream fan-out, tips, leader refresh, reconcile, and
    // crucially the timeout SWEEPER) run for as long as these handles + the
    // Arc<App> are alive — i.e. through the drain phase below.
    let _handles = app.spawn_background();
    info!("background tasks spawned");

    // Scope the drain to bundles created by THIS run.
    let baseline_id = app.max_bundle_id().await.unwrap_or(0);

    // Phase 1 — submit. Ctrl-c here exits promptly (no drain).
    let driver = Arc::clone(&app);
    let interrupted = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            warn!("ctrl-c received during submissions; exiting");
            true
        }
        res = drive_mode(&driver, mode) => {
            match res {
                Ok(()) => info!("submission phase complete"),
                Err(err) => warn!(error = %err, "submission phase ended with error"),
            }
            false
        }
    };
    if interrupted {
        info!("shutting down");
        return Ok(());
    }

    // Phase 2 — drain: poll until every bundle is terminal (Finalized/Failed) or
    // the cap elapses. The sweeper keeps marking never-landed bundles → Failed.
    // Ctrl-c cancels the drain and exits promptly (doesn't wait the full cap).
    info!(
        cap_secs = drain_timeout.as_secs(),
        "draining: waiting for all bundles to reach a terminal state (ctrl-c to exit now)..."
    );
    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("ctrl-c received; exiting now"),
        () = app.drain_to_terminal(baseline_id, drain_timeout) => {}
    }
    info!("shutting down");
    Ok(())
}

async fn drive_mode(app: &Arc<App>, mode: LiveMode) -> anyhow::Result<()> {
    match mode {
        LiveMode::Run {
            count,
            interval_slots,
        } => run_count(app, count, interval_slots).await,
        #[cfg(feature = "fault-injection")]
        LiveMode::Fault(kind) => match kind {
            FaultKind::StaleBlockhash => fault_stale_blockhash(app).await,
            FaultKind::SubFloorTip => fault_sub_floor_tip(app).await,
        },
    }
}

async fn run_count(app: &Arc<App>, count: u32, interval_slots: u64) -> anyhow::Result<()> {
    app.await_ready(Duration::from_secs(120)).await?;
    for i in 1..=count {
        let memo = format!("bundle-{i}-{}", now_nanos());
        info!(n = i, total = count, "submitting normal bundle");
        app.submit_one(app::base_spec(0, memo)).await?;
        if i < count {
            info!(interval_slots, "waiting before next submission");
            app::wait_slots(&app.slot_clock, interval_slots).await;
        }
    }
    info!(count, "all normal bundles submitted");
    Ok(())
}

/// Dispatch the fault command, refusing cleanly when the feature is off.
async fn run_fault(config: Config, kind: FaultKind) -> anyhow::Result<()> {
    #[cfg(not(feature = "fault-injection"))]
    {
        let _ = (config, kind);
        anyhow::bail!(
            "fault injection is disabled — rebuild with `--features fault-injection` to run `fault` modes"
        );
    }
    #[cfg(feature = "fault-injection")]
    {
        run_live(config, LiveMode::Fault(kind)).await
    }
}

#[cfg(feature = "fault-injection")]
async fn fault_stale_blockhash(app: &Arc<App>) -> anyhow::Result<()> {
    app.await_ready(Duration::from_secs(120)).await?;
    let start = app.slot_clock.load(std::sync::atomic::Ordering::Relaxed);
    let target = start + 156; // blockhash age > 155 slots
    info!(
        start_slot = start,
        target_slot = target,
        "stale-blockhash fault: holding submission until blockhash age > 155 slots"
    );

    let mut last_logged = u64::MAX;
    loop {
        let now = app.slot_clock.load(std::sync::atomic::Ordering::Relaxed);
        if now >= target {
            break;
        }
        let remaining = target - now;
        // Log the countdown roughly every 25 slots.
        if remaining / 25 != last_logged / 25 {
            info!(remaining_slots = remaining, current_slot = now, "stale-blockhash countdown");
            last_logged = remaining;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    info!("countdown complete; submitting bundle with StaleBlockhash fault");
    let mut spec = app::base_spec(0, format!("fault-stale-{}", now_nanos()));
    spec.fault = Some(submitter::Fault::StaleBlockhash { age_slots: 160 });
    app.submit_one(spec).await?;
    info!("stale-blockhash bundle submitted; expecting never-land -> timeout sweeper -> agent loop");
    Ok(())
}

#[cfg(feature = "fault-injection")]
async fn fault_sub_floor_tip(app: &Arc<App>) -> anyhow::Result<()> {
    app.await_ready(Duration::from_secs(120)).await?;
    let mut spec = app::base_spec(0, format!("fault-subfloor-{}", now_nanos()));
    spec.fault = Some(submitter::Fault::SubFloorTip { lamports: 500 });
    info!("submitting bundle with SubFloorTip{{500}} fault — expecting Block Engine rejection");
    // The raw rejection string is captured prominently inside submit_one.
    app.submit_one(spec).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// export / status (DB-only)
// ---------------------------------------------------------------------------

async fn export(config: Config) -> anyhow::Result<()> {
    let pool = app::open_pool(&config.db_path).await?;
    // Ensure the schema exists (no-op if already migrated).
    let lifecycle = LifecycleTracker::new(pool.clone(), LifecycleConfig::default()).await?;
    let paths = lifecycle
        .export_log(std::path::Path::new("./exports"))
        .await?;
    println!("Exported lifecycle log:");
    println!("  {}", paths.json.display());
    println!("  {}", paths.md.display());
    Ok(())
}

async fn status(config: Config) -> anyhow::Result<()> {
    let pool = app::open_pool(&config.db_path).await?;
    // Ensure the table exists so the query doesn't fail on a fresh DB.
    let _ = LifecycleTracker::new(pool.clone(), LifecycleConfig::default()).await?;
    app::print_status(&pool).await
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
