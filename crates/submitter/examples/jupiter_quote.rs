//! Prove we can get a real Jupiter route quote and a signable swap transaction —
//! the first step toward a bundle with real economic content (see `FINDINGS.md`).
//!
//! Fetches a quote for swapping 0.01 SOL → USDC, prints the route / expected out
//! amount / price impact, then asks Jupiter for the swap transaction and confirms
//! a base64 `VersionedTransaction` came back (decoding it to show it is real and
//! signable). Does NOT sign or submit anything on-chain.
//!
//! ```text
//! cargo run --example jupiter_quote -p submitter
//! ```
//! Env (from `.env`): `WALLET_KEYPAIR_PATH` (or `WALLET_PATH`). Optional:
//! `JUPITER_API_KEY` (uses the pro host) and `SWAP_SOL` (default 0.01).

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use solana_sdk::signature::Signer;
use solana_sdk::transaction::VersionedTransaction;
use submitter::jupiter::{JupiterClient, PRO_API_BASE, USDC_MINT, WSOL_MINT};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime::init_crypto();
    let _ = dotenvy::dotenv();

    let wallet_path = std::env::var("WALLET_KEYPAIR_PATH")
        .or_else(|_| std::env::var("WALLET_PATH"))
        .map_err(|_| anyhow::anyhow!("set WALLET_KEYPAIR_PATH (or WALLET_PATH)"))?;
    let keypair = solana_sdk::signature::read_keypair_file(&wallet_path)
        .map_err(|e| anyhow::anyhow!("read keypair {wallet_path}: {e}"))?;
    let user = keypair.pubkey();

    let sol: f64 = std::env::var("SWAP_SOL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.01);
    let amount_lamports = (sol * LAMPORTS_PER_SOL) as u64;
    let slippage_bps: u16 = 50;

    // Lite host by default; pro host if an API key is provided.
    let client = match std::env::var("JUPITER_API_KEY").ok().filter(|k| !k.is_empty()) {
        Some(key) => JupiterClient::with_base(PRO_API_BASE, Some(key)),
        None => JupiterClient::new(),
    };

    println!("wallet:       {user}");
    println!("swap:         {sol} SOL ({amount_lamports} lamports) -> USDC");
    println!("slippage_bps: {slippage_bps}\n");

    // 1. Quote.
    let quote = client
        .fetch_quote(WSOL_MINT, USDC_MINT, amount_lamports, slippage_bps)
        .await?;

    let route = quote.route_labels().join(" -> ");
    let out_amount = quote.out_amount().unwrap_or_else(|| "?".into());
    let out_usdc = out_amount.parse::<f64>().map(|v| v / 1e6).unwrap_or(f64::NAN);
    println!("=== QUOTE ===");
    println!("route:           {}", if route.is_empty() { "(none)".into() } else { route });
    println!("in amount:       {} lamports", quote.in_amount().unwrap_or_default());
    println!("out amount:      {out_amount} USDC base units (~{out_usdc:.6} USDC)");
    println!("price impact:    {}", quote.price_impact_pct().unwrap_or_else(|| "?".into()));
    println!("slippage (bps):  {}\n", quote.slippage_bps().unwrap_or_default());

    // 2. Swap transaction.
    let swap_b64 = client.fetch_swap_transaction(&quote, &user.to_string(), true).await?;
    println!("=== SWAP TRANSACTION ===");
    println!("swap transaction returned: yes");
    println!("base64 length:             {} chars", swap_b64.len());

    // Decode to prove it is a real, signable VersionedTransaction (NOT signed here).
    match BASE64
        .decode(swap_b64.as_bytes())
        .map_err(|e| anyhow::anyhow!("base64: {e}"))
        .and_then(|bytes| {
            bincode::deserialize::<VersionedTransaction>(&bytes)
                .map_err(|e| anyhow::anyhow!("bincode: {e}"))
        }) {
        Ok(tx) => {
            let required = tx.message.header().num_required_signatures as usize;
            println!("decoded:                   VersionedTransaction ({:?})", version_label(&tx));
            println!("accounts:                  {}", tx.message.static_account_keys().len());
            println!("required signatures:       {required} (unsigned here)");
        }
        Err(e) => println!("decode check failed:       {e}"),
    }

    println!("\n(no signing, no submission — quote + swap-tx fetch only)");
    Ok(())
}

fn version_label(tx: &VersionedTransaction) -> &'static str {
    use solana_sdk::message::VersionedMessage;
    match tx.message {
        VersionedMessage::Legacy(_) => "legacy",
        VersionedMessage::V0(_) => "v0",
        _ => "versioned",
    }
}
