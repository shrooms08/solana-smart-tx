//! Live smoke test for the Jito leader tracker.
//!
//! Wires the `stream` Yellowstone client into a [`LeaderTracker`] against real
//! infra: the stream feeds the slot clock, the tracker fetches the leader
//! schedule (RPC) + Jito validator set (kobe). It prints the current window
//! every second and announces each time a Jito leader window opens.
//!
//! Env (from `.env`, same vars the orchestrator's `Config` reads):
//!   * `RPC_URL`               — Solana JSON-RPC (for `getSlotLeaders`)
//!   * `YELLOWSTONE_ENDPOINT`  — Yellowstone gRPC endpoint (slot clock)
//!   * `YELLOWSTONE_X_TOKEN`   — optional Yellowstone auth token
//!
//! Run with:
//! ```text
//! cargo run --example leader_probe -p leader
//! ```

use std::time::Duration;

use leader::{LeaderConfig, LeaderTracker, RpcJitoSource};
use stream::{StreamClient, StreamConfig, StreamEvent};
use tokio::sync::mpsc;

const EVENT_CHANNEL_CAPACITY: usize = 1024;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the rustls CryptoProvider before any TLS client connects.
    runtime::init_crypto();
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let rpc_url = std::env::var("RPC_URL")
        .map_err(|_| anyhow::anyhow!("set RPC_URL in your environment / .env"))?;
    let endpoint = std::env::var("YELLOWSTONE_ENDPOINT")
        .map_err(|_| anyhow::anyhow!("set YELLOWSTONE_ENDPOINT in your environment / .env"))?;
    let x_token = std::env::var("YELLOWSTONE_X_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());

    // --- stream: feeds the slot clock (empty monitored set; slots only) ---
    let (event_tx, event_rx) = mpsc::channel::<StreamEvent>(EVENT_CHANNEL_CAPACITY);
    let stream_cfg = StreamConfig { endpoint, x_token };
    tokio::spawn(StreamClient::run(stream_cfg, Vec::new(), event_tx));

    // --- tracker: schedule (RPC) + Jito set (kobe), fed by the stream clock ---
    let tracker = LeaderTracker::new(LeaderConfig::default(), RpcJitoSource::mainnet(rpc_url));
    tracker.spawn_refresh();
    tokio::spawn(tracker.clone().ingest_loop(event_rx));

    // --- announce each time a Jito leader window opens (deduped per target) ---
    {
        let announce = tracker.clone();
        tokio::spawn(async move {
            let mut last_announced: Option<u64> = None;
            loop {
                match announce.wait_for_window().await {
                    Ok(window) => {
                        if last_announced != Some(window.next_jito_leader_slot) {
                            println!(
                                "🔔 JITO WINDOW OPEN — slot {} (in {} slots), leader {} [{}]",
                                window.next_jito_leader_slot,
                                window.slots_until,
                                window.leader_identity.as_deref().unwrap_or("?"),
                                if window.is_bam { "BAM" } else { "Block Engine" },
                            );
                            last_announced = Some(window.next_jito_leader_slot);
                        }
                    }
                    Err(err) => {
                        // Health state (warming up / stale); back off and retry.
                        eprintln!("(window wait health: {err})");
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }

    // --- status: print the current window every second ---
    println!("Probing leader windows (Ctrl-C to stop)...\n");
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    loop {
        ticker.tick().await;
        match tracker.current_window().await {
            Ok(w) => println!(
                "slot {:>12}  next_jito {:>12}  in {:>3} slots  leader {} [{}]",
                w.current_slot,
                w.next_jito_leader_slot,
                w.slots_until,
                w.leader_identity.as_deref().unwrap_or("?"),
                if w.is_bam { "BAM" } else { "BE" },
            ),
            Err(err) => println!("slot ?            health: {err}"),
        }
    }
}
