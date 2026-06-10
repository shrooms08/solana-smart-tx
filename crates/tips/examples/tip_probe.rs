//! Live smoke test for the Jito tip feed.
//!
//! Runs [`TipTracker`] against the public Jito endpoints (no credentials),
//! printing each snapshot (percentiles in lamports + freshness + which source
//! it came from) and the 60s trend. Kill the WebSocket / your network mid-run to
//! watch it fall back to REST polling (the `source` column flips to `rest`).
//!
//! Run with:
//! ```text
//! cargo run --example tip_probe -p tips
//! ```

use std::time::Duration;

use tips::{LiveTransport, TipConfig, TipTracker, TIP_FLOOR_REST_URL, TIP_STREAM_WS_URL};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tips=debug".into()),
        )
        .init();

    // URLs default to the live feeds; override TIP_WS_URL (e.g. to a dead host)
    // to demonstrate the REST fallback live.
    let ws_url = std::env::var("TIP_WS_URL").unwrap_or_else(|_| TIP_STREAM_WS_URL.to_string());
    let rest_url = std::env::var("TIP_REST_URL").unwrap_or_else(|_| TIP_FLOOR_REST_URL.to_string());
    let tracker = TipTracker::new(
        TipConfig::default(),
        LiveTransport::with_urls(ws_url, rest_url),
    );

    // Drive the feed in the background; it reconnects / falls back on its own.
    {
        let driver = tracker.clone();
        tokio::spawn(async move {
            if let Err(err) = driver.run().await {
                eprintln!("tip tracker stopped: {err}");
            }
        });
    }

    println!("Probing Jito tip feed for ~60s (percentiles in lamports)...\n");

    // Print once per second for ~65s so the live smoke test terminates cleanly.
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    let mut last_printed_at = None;
    for _ in 0..65 {
        ticker.tick().await;

        let Some(snap) = tracker.latest() else {
            println!("(waiting for first snapshot...)");
            continue;
        };

        // Skip duplicate prints when no new snapshot arrived this second.
        if last_printed_at == Some(snap.taken_at) {
            continue;
        }
        last_printed_at = Some(snap.taken_at);

        let source = tracker
            .latest_source()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".into());
        let freshness = tracker
            .freshness()
            .map(|d| format!("{:.1}s", d.as_secs_f64()))
            .unwrap_or_else(|| "?".into());
        let stale = if tracker.is_stale() { " STALE" } else { "" };

        let trend = tracker
            .trend(Duration::from_secs(60))
            .map(|t| {
                format!(
                    "p50 {:+} / p75 {:+} ({})",
                    t.p50_change_lamports,
                    t.p75_change_lamports,
                    if t.rising { "rising" } else { "flat/falling" }
                )
            })
            .unwrap_or_else(|| "(warming up)".into());

        println!(
            "[{source:>9}] p25={:>7} p50={:>7} p75={:>7} p95={:>8} p99={:>8} ema50={:>7}  age={freshness:>5}{stale}  60s-trend: {trend}",
            snap.p25_lamports,
            snap.p50_lamports,
            snap.p75_lamports,
            snap.p95_lamports,
            snap.p99_lamports,
            snap.ema_p50_lamports,
        );
    }

    println!("\nProbe complete.");
    Ok(())
}
