//! Live pre-submission bundle simulation — the definitive "would this bundle
//! abort?" probe. Builds the exact bundle `submit()` would (memo + tip transfer
//! to a freshly-fetched Jito tip account) and runs `simulateTransaction` on each
//! tx against the configured RPC, printing the error / logs / compute units.
//!
//! FAITHFUL mode: `sigVerify: true` + `replaceRecentBlockhash: false` — tests our
//! EXACT signed bytes against OUR real blockhash, so a stale blockhash surfaces
//! as BlockhashNotFound and a bad signature as a sig-verify error.
//!
//! Does NOT submit. Run with:
//! ```text
//! cargo run --example simulate_bundle -p submitter
//! ```
//! Env (from `.env`): `RPC_URL`, `JITO_BLOCK_ENGINE_URL`, `WALLET_PATH`
//! (or `WALLET_KEYPAIR_PATH`). Optional: `SIM_TIP_LAMPORTS` (default 100000).

use solana_sdk::signature::Signer;
use submitter::{BundleSpec, BundleSubmitter, SubmitterConfig, TipAccountStrategy};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime::init_crypto();
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,submitter=debug".into()),
        )
        .init();

    let rpc_url = std::env::var("RPC_URL").map_err(|_| anyhow::anyhow!("set RPC_URL"))?;
    let block_engine_url =
        std::env::var("JITO_BLOCK_ENGINE_URL").map_err(|_| anyhow::anyhow!("set JITO_BLOCK_ENGINE_URL"))?;
    let wallet_path = std::env::var("WALLET_PATH")
        .or_else(|_| std::env::var("WALLET_KEYPAIR_PATH"))
        .map_err(|_| anyhow::anyhow!("set WALLET_PATH (or WALLET_KEYPAIR_PATH)"))?;
    let tip_lamports: u64 = std::env::var("SIM_TIP_LAMPORTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);

    let keypair = solana_sdk::signature::read_keypair_file(&wallet_path)
        .map_err(|e| anyhow::anyhow!("read keypair {wallet_path}: {e}"))?;
    println!("wallet:           {}", keypair.pubkey());
    println!("block_engine_url: {block_engine_url}");
    println!("tip_lamports:     {tip_lamports}\n");

    let rpc = solana_client::nonblocking::rpc_client::RpcClient::new(rpc_url);
    let config = SubmitterConfig {
        block_engine_url,
        memo_prefix: "stx:".to_string(),
        tip_account_strategy: TipAccountStrategy::Random,
        self_transfer_lamports: std::env::var("SELF_TRANSFER_LAMPORTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_000),
        jito_rps: 1,
        auth_uuid: std::env::var("JITO_AUTH_UUID").ok(),
    };
    let submitter = BundleSubmitter::new(config, rpc, keypair);

    let spec = BundleSpec {
        tip_lamports,
        memo_text: "simulate-probe".to_string(),
        priority_fee_microlamports: 0,
        priority_fee_cu_limit: 0,
        #[cfg(feature = "fault-injection")]
        fault: None,
    };

    println!("=== Simulating bundle (NOT submitting) ===");
    // Rough latency indicator: this call does fetch-blockhash + fetch-tip-account
    // + 2 faithful simulateTransaction round-trips against the configured RPC.
    let t = std::time::Instant::now();
    let sim = submitter.simulate_bundle(spec, 0).await?;
    println!("(simulate_bundle round-trips took {} ms)\n", t.elapsed().as_millis());
    println!("blockhash:    {}", sim.blockhash);
    println!("tip_account:  {}", sim.tip_account);
    println!("tip_lamports: {}", sim.tip_lamports);

    let mut all_ok = true;
    for tx in &sim.transactions {
        println!("\n--- tx: {} (sig {}) ---", tx.label, tx.signature);
        match &tx.result.err {
            Some(err) => {
                all_ok = false;
                println!("  RESULT: FAILED — {err}");
            }
            None => println!("  RESULT: OK (no execution error)"),
        }
        println!("  units_consumed: {:?}", tx.result.units_consumed);
        println!("  logs ({}):", tx.result.logs.len());
        for line in &tx.result.logs {
            println!("    {line}");
        }
    }

    println!(
        "\n=== VERDICT: {} ===",
        if all_ok {
            "both txs pass FAITHFUL sim (real sigs + real blockhash valid) — bytes/blockhash are NOT the cause"
        } else {
            "a tx FAILS faithful sim — check above for BlockhashNotFound (stale) or signature error"
        }
    );
    Ok(())
}
