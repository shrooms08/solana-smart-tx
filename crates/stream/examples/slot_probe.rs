//! Live smoke test for the Yellowstone stream client.
//!
//! Loads the Yellowstone endpoint + token from `.env` (the same vars the
//! orchestrator's `Config` reads), spins up [`StreamClient::run`] with an empty
//! monitored-pubkey list, consumes [`StreamEvent`]s off the channel, and prints
//! the first ~30 slot updates: slot number, status, and a running
//! processed -> confirmed latency delta per slot.
//!
//! Run it against real infra with:
//! ```text
//! cargo run --example slot_probe -p stream
//! ```
//! (Set `YELLOWSTONE_ENDPOINT` and, if your provider needs it,
//! `YELLOWSTONE_X_TOKEN` in `.env` first.)

use std::collections::HashMap;
use std::time::SystemTime;

use stream::{SlotStatus, StreamClient, StreamConfig, StreamEvent};
use tokio::sync::mpsc;

/// How many slot updates to print before we consider the probe a success.
const TARGET_SLOTS: usize = 30;
/// Bounded event channel depth — small on purpose so backpressure is realistic.
const CHANNEL_CAPACITY: usize = 1024;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the rustls CryptoProvider before any TLS client connects.
    runtime::init_crypto();
    // Best-effort .env load; real env vars still win.
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let endpoint = std::env::var("YELLOWSTONE_ENDPOINT")
        .map_err(|_| anyhow::anyhow!("set YELLOWSTONE_ENDPOINT in your environment / .env"))?;
    // Treat an empty token as "no token".
    let x_token = std::env::var("YELLOWSTONE_X_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());

    let config = StreamConfig { endpoint, x_token };

    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(CHANNEL_CAPACITY);

    // Empty monitored set: we only care about slot updates for the smoke test.
    // (Swap in a single pubkey here to also see tx-status events for a wallet.)
    let monitored = Vec::new();

    // Drive the client in the background; it loops/reconnects on its own.
    let client = tokio::spawn(StreamClient::run(config, monitored, event_tx));

    println!("Probing Yellowstone — waiting for the first {TARGET_SLOTS} slot updates...\n");

    // Per-slot timestamp of the first `Processed` we saw, to compute the
    // processed -> confirmed delta when the slot later confirms.
    let mut processed_at: HashMap<u64, SystemTime> = HashMap::new();
    let mut printed = 0usize;

    while let Some(event) = event_rx.recv().await {
        match event {
            StreamEvent::Slot(slot) => {
                let delta = match slot.status {
                    SlotStatus::Processed => {
                        processed_at.entry(slot.slot).or_insert(slot.received_at);
                        None
                    }
                    SlotStatus::Confirmed => processed_at
                        .get(&slot.slot)
                        .and_then(|p| slot.received_at.duration_since(*p).ok()),
                    _ => None,
                };

                let delta_str = match delta {
                    Some(d) => format!("  (processed->confirmed: {:>6.1} ms)", d.as_secs_f64() * 1e3),
                    None => String::new(),
                };

                println!(
                    "slot {:>12}  status={:<10}{}",
                    slot.slot,
                    format!("{:?}", slot.status),
                    delta_str
                );

                printed += 1;
                if printed >= TARGET_SLOTS {
                    break;
                }
            }
            StreamEvent::TxStatus(tx) => {
                println!(
                    "tx   {:<88}  slot={:>12}  err={:?}",
                    tx.signature, tx.slot, tx.err
                );
            }
        }
    }

    println!("\nProbe complete: saw {printed} slot updates. Shutting down.");
    // Dropping event_rx signals the client to stop (ConsumerGone); abort to be
    // immediate rather than waiting for the next update.
    client.abort();
    Ok(())
}
