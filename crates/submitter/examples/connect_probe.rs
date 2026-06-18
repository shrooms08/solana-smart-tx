//! Localize the send latency: measure COLD (DNS+TCP+TLS handshake + round-trip)
//! vs WARM (round-trip only, connection reused) requests to the Jito block engine
//! over a SINGLE pooled reqwest client — the same client/host `send_bundle` uses.
//!
//! The split tells us how much of the ~742ms send is a fixable TLS handshake vs
//! the physical network round-trip to the region. Probes `getTipAccounts` (same
//! host as `sendBundle`, cheap, read-only). Does NOT submit anything.
//!
//! ```text
//! cargo run --example connect_probe -p submitter
//! ```
//! Env (from `.env`): `JITO_BLOCK_ENGINE_URL`.

use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime::init_crypto();
    let _ = dotenvy::dotenv();

    let block_engine_url = std::env::var("JITO_BLOCK_ENGINE_URL")
        .map_err(|_| anyhow::anyhow!("set JITO_BLOCK_ENGINE_URL"))?;
    let url = format!(
        "{}/api/v1/getTipAccounts",
        block_engine_url.trim_end_matches('/')
    );
    println!("block engine: {block_engine_url}");
    println!("probe url:    {url}\n");

    // Same client config as the submitter's LiveGateway (keep-alive pool).
    let http = reqwest::Client::builder()
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(4)
        .tcp_keepalive(Duration::from_secs(30))
        .build()?;
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "getTipAccounts", "params": []
    });

    let call = |http: reqwest::Client, url: String, body: serde_json::Value| async move {
        let t = Instant::now();
        let resp = http.post(&url).json(&body).send().await?;
        let _ = resp.text().await?; // include response read
        Ok::<u128, anyhow::Error>(t.elapsed().as_millis())
    };

    // COLD: first request on a fresh client -> full DNS + TCP + TLS handshake.
    let cold_ms = call(http.clone(), url.clone(), body.clone()).await?;
    println!("COLD call (DNS+TCP+TLS handshake + round-trip): {cold_ms} ms");

    // WARM: subsequent requests reuse the pooled connection -> round-trip only.
    let mut warm = Vec::new();
    for i in 1..=5 {
        let ms = call(http.clone(), url.clone(), body.clone()).await?;
        println!("WARM call #{i} (connection reused, round-trip only): {ms} ms");
        warm.push(ms);
    }
    let warm_min = *warm.iter().min().unwrap();
    let warm_avg = warm.iter().sum::<u128>() / warm.len() as u128;

    let handshake = cold_ms.saturating_sub(warm_min);
    println!("\n=== BREAKDOWN ===");
    println!("cold (with handshake):        {cold_ms} ms");
    println!("warm round-trip (min / avg):  {warm_min} / {warm_avg} ms");
    println!("implied TLS handshake cost:   ~{handshake} ms  (FIXABLE via connection reuse / pre-warm)");
    println!("physical network round-trip:  ~{warm_min} ms  (region latency floor)");
    println!(
        "\nIf the orchestrator pre-warms this connection at startup, send_bundle pays only the\n\
         ~{warm_min} ms round-trip in the hot path instead of ~{cold_ms} ms."
    );
    Ok(())
}
