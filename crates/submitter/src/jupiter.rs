//! Jupiter swap API client — fetches a route quote and a signable swap
//! transaction. This is the first step toward giving the bundle real economic
//! content (a SOL→USDC→SOL round-trip) so it can win a Jito auction; see
//! `FINDINGS.md`. This module only talks to Jupiter — no bundle wiring, no signing,
//! no on-chain submission.
//!
//! ## Endpoints (verified against Jupiter's current API, June 2026)
//!
//! Jupiter migrated off the legacy `quote-api.jup.ag/v6`. The current structure is
//! `/<product>/v1` under two hosts:
//!   * **free / lite** — `https://lite-api.jup.ag/swap/v1` (no API key; rate-limited)
//!   * **paid / pro**  — `https://api.jup.ag/swap/v1` (requires an `x-api-key` header)
//!
//! We default to the lite host. Routes used here:
//!   * `GET  /swap/v1/quote` — route + amounts + price impact
//!   * `POST /swap/v1/swap`  — a base64 `VersionedTransaction` ready to sign
//!
//! The full quote object must be passed back to `/swap` **verbatim**, so
//! [`Quote`] retains the raw JSON and re-submits it unmodified.

use std::time::Duration;

use serde_json::Value;

/// USD Coin SPL mint.
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
/// Wrapped SOL (native mint).
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

/// Free/lite Jupiter swap API base (no API key required).
pub const LITE_API_BASE: &str = "https://lite-api.jup.ag/swap/v1";
/// Paid/pro Jupiter swap API base (requires an `x-api-key` header).
pub const PRO_API_BASE: &str = "https://api.jup.ag/swap/v1";

/// A Jupiter route quote. Holds the **complete** `/quote` response so it can be
/// handed back to `/swap` byte-for-byte (Jupiter rejects a partial quote), with
/// typed accessors for the fields we display.
#[derive(Debug, Clone)]
pub struct Quote {
    /// The full, untouched `/quote` JSON response.
    pub value: Value,
}

impl Quote {
    /// Input mint (base58).
    pub fn input_mint(&self) -> Option<String> {
        field_str(&self.value, "inputMint")
    }
    /// Output mint (base58).
    pub fn output_mint(&self) -> Option<String> {
        field_str(&self.value, "outputMint")
    }
    /// Input amount, in the input mint's base units (Jupiter returns it as a string).
    pub fn in_amount(&self) -> Option<String> {
        field_str(&self.value, "inAmount")
    }
    /// Expected output amount, in the output mint's base units.
    pub fn out_amount(&self) -> Option<String> {
        field_str(&self.value, "outAmount")
    }
    /// Price impact as a fraction (e.g. "0.0012" = 0.12%); may be the string "0".
    pub fn price_impact_pct(&self) -> Option<String> {
        field_str(&self.value, "priceImpactPct")
    }
    /// Effective slippage tolerance in basis points.
    pub fn slippage_bps(&self) -> Option<u64> {
        self.value.get("slippageBps").and_then(Value::as_u64)
    }
    /// The AMM labels along the route, in order (e.g. `["Invariant"]`,
    /// `["Orca", "Raydium"]`).
    pub fn route_labels(&self) -> Vec<String> {
        self.value
            .get("routePlan")
            .and_then(Value::as_array)
            .map(|steps| {
                steps
                    .iter()
                    .filter_map(|s| field_str(s.get("swapInfo")?, "label"))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A pooled client for Jupiter's swap API. Uses the same keep-alive/pooling
/// configuration as the block-engine client so we don't re-handshake TLS per call.
/// Cloning shares the underlying connection pool (reqwest::Client is `Arc` inside),
/// so a background pre-fetcher and the hot path reuse the same warm connection.
#[derive(Clone)]
pub struct JupiterClient {
    http: reqwest::Client,
    base_url: String,
    /// Optional `x-api-key` (only needed for the pro host).
    api_key: Option<String>,
}

impl Default for JupiterClient {
    fn default() -> Self {
        Self::new()
    }
}

impl JupiterClient {
    /// A client against the free/lite host, no API key.
    pub fn new() -> Self {
        Self::with_base(LITE_API_BASE, None)
    }

    /// A client against an explicit base URL, with an optional API key (required
    /// for [`PRO_API_BASE`]).
    pub fn with_base(base_url: &str, api_key: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(4)
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    /// `GET /quote` — fetch a route quote for swapping `amount` base units of
    /// `input_mint` into `output_mint` with `slippage_bps` slippage tolerance.
    pub async fn fetch_quote(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: u16,
    ) -> anyhow::Result<Quote> {
        let url = format!("{}/quote", self.base_url);
        let amount = amount.to_string();
        let slippage = slippage_bps.to_string();
        let mut req = self.http.get(&url).query(&[
            ("inputMint", input_mint),
            ("outputMint", output_mint),
            ("amount", amount.as_str()),
            ("slippageBps", slippage.as_str()),
        ]);
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }
        let value = send_json(req, "quote").await?;
        Ok(Quote { value })
    }

    /// `POST /swap` — exchange a [`Quote`] for a base64-encoded
    /// `VersionedTransaction` that, once signed by `user_pubkey`, executes the
    /// swap. `wrap_unwrap_sol` wraps/unwraps native SOL automatically (needed when
    /// one side of the swap is SOL). Returns the base64 transaction string; this
    /// function does NOT sign or submit it.
    pub async fn fetch_swap_transaction(
        &self,
        quote: &Quote,
        user_pubkey: &str,
        wrap_unwrap_sol: bool,
    ) -> anyhow::Result<String> {
        let url = format!("{}/swap", self.base_url);
        let body = serde_json::json!({
            "quoteResponse": quote.value,
            "userPublicKey": user_pubkey,
            "wrapAndUnwrapSol": wrap_unwrap_sol,
        });
        let mut req = self.http.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }
        let value = send_json(req, "swap").await?;
        value
            .get("swapTransaction")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("swap response missing 'swapTransaction': {value}"))
    }
}

/// Send a request, check the HTTP status, and parse the body as JSON — bailing
/// with the raw body on a non-2xx so Jupiter's error message survives.
async fn send_json(req: reqwest::RequestBuilder, what: &str) -> anyhow::Result<Value> {
    let resp = req
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("{what}: error sending request: {e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("{what}: error reading response body: {e}"))?;
    if !status.is_success() {
        anyhow::bail!("{what}: HTTP {}: {text}", status.as_u16());
    }
    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{what}: error decoding response: {e}"))
}

/// Read a JSON field as a display string whether it is stored as a string
/// (amounts, mints) or a bare number/bool (`priceImpactPct` is sometimes `0`).
fn field_str(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_are_the_canonical_values() {
        assert_eq!(USDC_MINT, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        assert_eq!(WSOL_MINT, "So11111111111111111111111111111111111111112");
    }

    #[test]
    fn quote_accessors_read_typed_and_raw_fields() {
        // A trimmed but representative /quote response shape.
        let value = serde_json::json!({
            "inputMint": WSOL_MINT,
            "inAmount": "10000000",
            "outputMint": USDC_MINT,
            "outAmount": "734112",
            "otherAmountThreshold": "730442",
            "swapMode": "ExactIn",
            "slippageBps": 50,
            "priceImpactPct": "0",
            "routePlan": [
                { "swapInfo": { "label": "Invariant", "ammKey": "x" }, "percent": 100 }
            ],
            "contextSlot": 123,
            // an unknown extra field must NOT break parsing or the round-trip
            "loadedLongtailToken": false
        });
        let q = Quote { value };
        assert_eq!(q.input_mint().as_deref(), Some(WSOL_MINT));
        assert_eq!(q.out_amount().as_deref(), Some("734112"));
        assert_eq!(q.price_impact_pct().as_deref(), Some("0"));
        assert_eq!(q.slippage_bps(), Some(50));
        assert_eq!(q.route_labels(), vec!["Invariant".to_string()]);
        // The raw value is retained verbatim for re-submission to /swap.
        assert_eq!(q.value.get("loadedLongtailToken").unwrap(), &Value::Bool(false));
    }

    #[test]
    fn field_str_handles_string_and_number() {
        let v = serde_json::json!({ "s": "abc", "n": 42, "z": null });
        assert_eq!(field_str(&v, "s").as_deref(), Some("abc"));
        assert_eq!(field_str(&v, "n").as_deref(), Some("42"));
        assert_eq!(field_str(&v, "z"), None);
        assert_eq!(field_str(&v, "missing"), None);
    }

    #[test]
    fn lite_client_builds_with_default_base() {
        let c = JupiterClient::new();
        assert_eq!(c.base_url, LITE_API_BASE);
        assert!(c.api_key.is_none());
        // Trailing slash is trimmed.
        let c2 = JupiterClient::with_base("https://api.jup.ag/swap/v1/", Some("k".into()));
        assert_eq!(c2.base_url, "https://api.jup.ag/swap/v1");
        assert_eq!(c2.api_key.as_deref(), Some("k"));
    }
}
