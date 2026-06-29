//! Swap-mode DRY RUN against the LIVE wallet + LIVE Jupiter API + LIVE RPC.
//!
//! Fetches a real Jupiter quote + swap tx for the configured swap (default
//! 0.01 SOL -> USDC), signs it as bundle tx0, builds the tip tx1 on a fresh
//! blockhash, decodes the assembled `[swap, tip]` bundle, and runs
//! `simulateTransaction` on the signed swap (sigVerify on, no blockhash
//! replacement). Proves the live Jupiter tx signs and would execute — BEFORE we
//! spend real SOL. Does NOT call sendBundle / submit anything to Jito.
//!
//! ```text
//! cargo run --example swap_dry_run -p submitter
//! ```
//! Env (from `.env`): `RPC_URL`, `JITO_BLOCK_ENGINE_URL`, `WALLET_KEYPAIR_PATH`
//! (or `WALLET_PATH`). Optional: `JITO_AUTH_UUID`, `SWAP_INPUT_MINT`,
//! `SWAP_OUTPUT_MINT`, `SWAP_AMOUNT_LAMPORTS` (default 10000000),
//! `SWAP_SLIPPAGE_BPS` (default 50), `SWAP_DRYRUN_TIP_LAMPORTS` (default 10000).

use solana_sdk::signature::Signer;
use submitter::jupiter::{JupiterClient, USDC_MINT, WSOL_MINT};
use submitter::{BundleSubmitter, SubmitterConfig, TipAccountStrategy};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime::init_crypto();
    let _ = dotenvy::dotenv();

    let rpc_url = std::env::var("RPC_URL").map_err(|_| anyhow::anyhow!("set RPC_URL"))?;
    let block_engine_url = std::env::var("JITO_BLOCK_ENGINE_URL")
        .map_err(|_| anyhow::anyhow!("set JITO_BLOCK_ENGINE_URL"))?;
    let wallet_path = std::env::var("WALLET_KEYPAIR_PATH")
        .or_else(|_| std::env::var("WALLET_PATH"))
        .map_err(|_| anyhow::anyhow!("set WALLET_KEYPAIR_PATH (or WALLET_PATH)"))?;
    let input_mint = env_or("SWAP_INPUT_MINT", WSOL_MINT);
    let output_mint = env_or("SWAP_OUTPUT_MINT", USDC_MINT);
    let amount: u64 = env_or("SWAP_AMOUNT_LAMPORTS", "10000000").parse()?;
    let slippage_bps: u16 = env_or("SWAP_SLIPPAGE_BPS", "50").parse()?;
    let tip_lamports: u64 = env_or("SWAP_DRYRUN_TIP_LAMPORTS", "10000").parse()?;

    let keypair = solana_sdk::signature::read_keypair_file(&wallet_path)
        .map_err(|e| anyhow::anyhow!("read keypair {wallet_path}: {e}"))?;
    let user = keypair.pubkey();
    println!("wallet:  {user}");
    println!("swap:    {amount} base units of {input_mint}");
    println!("         -> {output_mint}  (slippage {slippage_bps} bps)\n");

    // 1. LIVE Jupiter: quote + swap transaction (unsigned).
    let jup = JupiterClient::new();
    let quote = jup.fetch_quote(&input_mint, &output_mint, amount, slippage_bps).await?;
    let route = {
        let l = quote.route_labels();
        if l.is_empty() { "(direct)".into() } else { l.join(" -> ") }
    };
    println!("=== JUPITER QUOTE ===");
    println!("route:       {route}");
    println!("out amount:  {}", quote.out_amount().unwrap_or_default());
    println!("price impact:{}\n", quote.price_impact_pct().unwrap_or_default());
    let swap_b64 = jup.fetch_swap_transaction(&quote, &user.to_string(), true).await?;

    // 2. Submitter over the LIVE gateway — used ONLY for the dry run (no submit).
    let rpc = solana_client::nonblocking::rpc_client::RpcClient::new(rpc_url);
    let config = SubmitterConfig {
        block_engine_url,
        memo_prefix: "stx:".to_string(),
        tip_account_strategy: TipAccountStrategy::Random,
        self_transfer_lamports: 1_000,
        jito_rps: 1,
        auth_uuid: std::env::var("JITO_AUTH_UUID").ok().filter(|s| !s.is_empty()),
    };
    let submitter = BundleSubmitter::new(config, rpc, keypair);

    // 3. DRY RUN: sign swap -> tx0, build tip -> tx1, decode + simulate tx0. No send.
    let dry = submitter
        .dry_run_swap_bundle(&swap_b64, tip_lamports, 0, true)
        .await?;

    println!("=== DECODED BUNDLE (assembled, NOT sent) ===");
    println!("tx0 (Jupiter swap):");
    println!("  version:        {}", dry.tx0_version);
    println!("  signed:         {}", dry.tx0_signed);
    println!("  signature:      {}", dry.tx0_signature);
    println!("  account count:  {}", dry.tx0_num_accounts);
    println!("  blockhash:      {}", dry.tx0_blockhash);
    println!("tx1 (tip transfer):");
    println!("  tip account:    {}", dry.tip_account);
    println!("  tip lamports:   {}", dry.tip_lamports);
    println!("  signature:      {}", dry.tip_signature);
    println!("  blockhash:      {}", dry.tip_blockhash);
    println!(
        "blockhashes differ (independent txs): {}\n",
        dry.blockhashes_differ
    );

    println!("=== SWAP TX (tx0) SIMULATION (sigVerify=true, replaceRecentBlockhash=false) ===");
    match &dry.tx0_simulation {
        Some(sim) => match &sim.err {
            None => {
                println!("result:         CLEAN (no error)");
                println!("compute units:  {:?}", sim.units_consumed);
                println!("log lines:      {}", sim.logs.len());
            }
            Some(err) => {
                println!("result:         ERROR -> {err}");
                println!("compute units:  {:?}", sim.units_consumed);
                for line in &sim.logs {
                    println!("  log: {line}");
                }
            }
        },
        None => println!("(simulation not requested)"),
    }

    println!("\n(no sendBundle — dry run only; no SOL spent)");
    Ok(())
}
