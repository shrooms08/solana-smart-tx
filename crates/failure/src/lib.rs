//! Failure classification for the smart-transaction stack.
//!
//! Turns heterogeneous failure evidence — Block Engine `sendBundle` rejections,
//! on-chain `TransactionError`s decoded off the stream, and bundles that simply
//! never landed — into the four bounty classes ([`FailureKind`]), while
//! honestly representing uncertainty via [`Confidence`].
//!
//! Everything here is **pure**: [`classify`] takes a borrowed [`Evidence`] and
//! returns a [`Classification`]. No I/O, no async, no clocks.

use serde::{Deserialize, Serialize};
use solana_instruction_error::InstructionError;
use solana_transaction_error::TransactionError;

/// Recent-blockhash validity window, in slots. A blockhash older than this at
/// the last observation can no longer be used to land a transaction.
const BLOCKHASH_VALIDITY_SLOTS: u64 = 150;

// ---------------------------------------------------------------------------
// Public taxonomy
// ---------------------------------------------------------------------------

/// The canonical reason a transaction / bundle did not land.
///
/// Kept deliberately coarse: each variant should map to a distinct corrective
/// action in the decision layer (see `agent::Decision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailureKind {
    /// The recent blockhash referenced by the transaction is no longer valid.
    ExpiredBlockhash,
    /// Priority fee / Jito tip was too low to be included.
    FeeTooLow,
    /// The transaction exceeded its compute-unit budget.
    ComputeExceeded,
    /// The Jito bundle itself failed (dropped, not selected, sim failure, ...).
    BundleFailure,
    /// The submission never reached the auction: a tip-account fetch failure, a
    /// `sendBundle` network/transport error, or an empty/malformed Block Engine
    /// response *before* the bundle was accepted. The bundle, blockhash, and tip
    /// are not implicated — the infrastructure was unreachable.
    TransportError,
    /// The bundle was accepted by the Block Engine (a `bundle_id` was returned)
    /// but never won its auction / never landed — confirmed by Jito's
    /// `getInflightBundleStatuses` returning `Invalid`/`Failed`, or inferred when a
    /// bundle with a valid-at-submission blockhash and a competitive tip simply
    /// never landed. The blockhash aging past validity afterwards is a *downstream
    /// symptom* of sitting unlanded, NOT the cause — so this is distinct from
    /// [`ExpiredBlockhash`](Self::ExpiredBlockhash).
    AuctionLost,
}

/// The last Jito `getInflightBundleStatuses` verdict observed for a bundle, as
/// seen by the classifier. Mirrors `submitter::BundleStatus` but kept independent
/// so the `failure` crate has no dependency on `submitter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JitoInflight {
    /// `getInflightBundleStatuses` returned `Invalid` — not in Jito's system /
    /// never entered the auction (definitive auction loss).
    Invalid,
    /// `Failed` — all regions failed/expired or it landed elsewhere (lost).
    Failed,
    /// `Pending` — was in the system but had not been processed when last polled.
    Pending,
    /// `Landed` — Jito reported it on-chain (should not normally reach NeverLanded).
    Landed,
    /// Polled but the status string was unrecognized.
    Unknown,
    /// Never polled / no status recorded for this bundle.
    NotPolled,
}

impl JitoInflight {
    /// Map a recorded status string (e.g. from the bundle-status poller, or the
    /// persisted `jito_inflight_status` column) onto the enum. `None`/empty →
    /// `NotPolled`.
    pub fn from_status_str(s: Option<&str>) -> Self {
        match s.map(|v| v.trim().to_ascii_lowercase()) {
            Some(v) if v == "invalid" => Self::Invalid,
            Some(v) if v == "failed" => Self::Failed,
            Some(v) if v == "pending" => Self::Pending,
            Some(v) if v == "landed" => Self::Landed,
            Some(v) if v.is_empty() => Self::NotPolled,
            Some(_) => Self::Unknown,
            None => Self::NotPolled,
        }
    }
}

/// Heterogeneous evidence handed to the classifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Evidence {
    /// The Block Engine rejected the bundle at `sendBundle`.
    SubmitRejection {
        /// Raw rejection string from the Block Engine.
        raw_error: String,
    },
    /// The transaction landed on-chain but failed; `raw_error_hex` is the
    /// hex-encoded bincode `TransactionError` carried by the stream crate.
    OnChainError { raw_error_hex: String, slot: u64 },
    /// The bundle was submitted but never observed landing. The probabilistic
    /// door — we infer the most plausible cause from slot/tip context and the
    /// last Jito inflight status.
    NeverLanded {
        submitted_slot: u64,
        blockhash_fetched_at_slot: u64,
        last_observed_slot: u64,
        tip_lamports: u64,
        tip_p50_at_submit: Option<u64>,
        tip_p75_at_submit: Option<u64>,
        /// Last `getInflightBundleStatuses` verdict from the bundle-status poller.
        /// `Invalid`/`Failed` is direct evidence the bundle lost its auction.
        jito_inflight: JitoInflight,
    },
}

/// How sure the classifier is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence {
    /// Unambiguous evidence (e.g. a decoded `BlockhashNotFound`).
    Certain,
    /// The most plausible single explanation, but not proven.
    Likely,
    /// Multiple explanations fit; `alternatives` lists the other plausible
    /// [`FailureKind`]s (may be empty when the cause is genuinely unknown).
    Ambiguous { alternatives: Vec<FailureKind> },
}

/// The result of classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    pub kind: FailureKind,
    pub confidence: Confidence,
    /// Human-readable justification, always citing the concrete evidence
    /// (decoded error, the raw string, or the actual slot/tip numbers).
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Classify a single piece of [`Evidence`]. Pure: no I/O, no async, no clocks.
pub fn classify(evidence: &Evidence) -> Classification {
    match evidence {
        Evidence::SubmitRejection { raw_error } => classify_submit_rejection(raw_error),
        Evidence::OnChainError {
            raw_error_hex,
            slot,
        } => classify_on_chain(raw_error_hex, *slot),
        Evidence::NeverLanded {
            submitted_slot,
            blockhash_fetched_at_slot,
            last_observed_slot,
            tip_lamports,
            tip_p50_at_submit,
            tip_p75_at_submit,
            jito_inflight,
        } => classify_never_landed(
            *submitted_slot,
            *blockhash_fetched_at_slot,
            *last_observed_slot,
            *tip_lamports,
            *tip_p50_at_submit,
            *tip_p75_at_submit,
            *jito_inflight,
        ),
    }
}

// ---------------------------------------------------------------------------
// 1. Block Engine rejections
// ---------------------------------------------------------------------------

/// Pattern-match known Block Engine `sendBundle` rejection strings.
///
/// Matching is case-insensitive on substrings because the exact wording varies
/// across Block Engine / relayer versions; unknown strings fall back to
/// `BundleFailure` / `Likely` with the raw text preserved.
fn classify_submit_rejection(raw_error: &str) -> Classification {
    let lower = raw_error.to_lowercase();

    // Pre-submission transport / decode failure: the bundle never reached the
    // auction. This MUST take precedence over the auction-rejection patterns
    // below, so an infra failure is never mistaken for a tip/sim/malformed
    // rejection (which would wrongly push the agent to raise the tip).
    if looks_like_transport(&lower) {
        return Classification {
            kind: FailureKind::TransportError,
            confidence: Confidence::Certain,
            rationale: format!(
                "pre-submission transport/decode failure — bundle never reached the auction; \
                 the bundle, blockhash, and tip are not implicated: {raw_error:?}"
            ),
        };
    }

    // Tip below the Block Engine's minimum tip.
    if lower.contains("must tip at least")
        || (lower.contains("tip") && (lower.contains("too low") || lower.contains("below")))
    {
        return Classification {
            kind: FailureKind::FeeTooLow,
            confidence: Confidence::Certain,
            rationale: format!("Block Engine rejected bundle for low tip: {raw_error:?}"),
        };
    }

    // Bundle failed simulation (one of its transactions errored pre-inclusion).
    if lower.contains("simulation failure")
        || lower.contains("failed to simulate")
        || lower.contains("transaction failure")
    {
        return Classification {
            kind: FailureKind::BundleFailure,
            confidence: Confidence::Certain,
            rationale: format!("Block Engine reported bundle simulation failure: {raw_error:?}"),
        };
    }

    // Malformed / undecodable / oversized bundle.
    if lower.contains("malformed")
        || lower.contains("deserialize")
        || lower.contains("decode")
        || lower.contains("invalid")
        || lower.contains("too many")
    {
        return Classification {
            kind: FailureKind::BundleFailure,
            confidence: Confidence::Certain,
            rationale: format!("Block Engine rejected bundle as malformed/invalid: {raw_error:?}"),
        };
    }

    // Unknown rejection string: preserve it for the agent / a human.
    Classification {
        kind: FailureKind::BundleFailure,
        confidence: Confidence::Likely,
        rationale: format!("unrecognized Block Engine rejection: {raw_error:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2. On-chain TransactionError
// ---------------------------------------------------------------------------

fn classify_on_chain(raw_error_hex: &str, slot: u64) -> Classification {
    let bytes = match decode_hex(raw_error_hex) {
        Some(bytes) => bytes,
        None => {
            return Classification {
                kind: FailureKind::BundleFailure,
                confidence: Confidence::Ambiguous {
                    alternatives: Vec::new(),
                },
                rationale: format!(
                    "could not hex-decode on-chain error (slot {slot}); raw hex preserved: {raw_error_hex:?}"
                ),
            };
        }
    };

    match bincode::deserialize::<TransactionError>(&bytes) {
        Ok(tx_error) => map_tx_error(&tx_error, slot),
        Err(err) => Classification {
            kind: FailureKind::BundleFailure,
            confidence: Confidence::Ambiguous {
                alternatives: Vec::new(),
            },
            rationale: format!(
                "could not decode TransactionError (slot {slot}): {err}; raw hex preserved: {raw_error_hex:?}"
            ),
        },
    }
}

/// Map a decoded [`TransactionError`] to a [`FailureKind`].
///
/// Compute-mapped variants (→ `ComputeExceeded`, `Certain`):
///   * `InstructionError(_, InstructionError::ComputationalBudgetExceeded)` —
///     the instruction ran out of compute units.
///   * `TransactionError::MaxLoadedAccountsDataSizeExceeded` — the transaction
///     exceeded its loaded-accounts-data-size compute budget.
///
/// `BlockhashNotFound` → `ExpiredBlockhash` / `Certain`. Everything else →
/// `BundleFailure` / `Likely` with the `Debug` repr in the rationale.
fn map_tx_error(tx_error: &TransactionError, slot: u64) -> Classification {
    match tx_error {
        TransactionError::BlockhashNotFound => Classification {
            kind: FailureKind::ExpiredBlockhash,
            confidence: Confidence::Certain,
            rationale: format!("on-chain TransactionError::BlockhashNotFound at slot {slot}"),
        },

        TransactionError::InstructionError(ix, InstructionError::ComputationalBudgetExceeded) => {
            Classification {
                kind: FailureKind::ComputeExceeded,
                confidence: Confidence::Certain,
                rationale: format!(
                    "on-chain compute-budget exceeded at slot {slot}: instruction {ix} \
                     returned ComputationalBudgetExceeded"
                ),
            }
        }

        TransactionError::MaxLoadedAccountsDataSizeExceeded => Classification {
            kind: FailureKind::ComputeExceeded,
            confidence: Confidence::Certain,
            rationale: format!(
                "on-chain compute budget exceeded at slot {slot}: \
                 MaxLoadedAccountsDataSizeExceeded"
            ),
        },

        other => Classification {
            kind: FailureKind::BundleFailure,
            confidence: Confidence::Likely,
            rationale: format!("on-chain failure at slot {slot}: {other:?}"),
        },
    }
}

// ---------------------------------------------------------------------------
// 3. NeverLanded — the probabilistic door
// ---------------------------------------------------------------------------

/// Classify a never-landed timeout. Priority is chosen so the *root cause* is
/// named, never a downstream symptom:
///
/// 1. **Blockhash stale AT submission** (`submitted - fetched > validity`): a real
///    `ExpiredBlockhash` — we shipped an already-expired hash.
/// 2. **Tip below p50 at submit**: `FeeTooLow` — outbid (actionable: raise tip).
/// 3. **Jito inflight `Invalid`/`Failed`**: `AuctionLost` — the Block Engine
///    accepted the bundle (issued a `bundle_id`) but it never won/entered.
/// 4. Otherwise (valid-at-submit blockhash, competitive tip, not confirmed
///    Invalid): `AuctionLost` (inferred) — never landed despite being well-formed.
///
/// Critically, a blockhash that only aged past validity *while the bundle sat
/// unlanded* is a symptom of (3)/(4), NOT `ExpiredBlockhash` — that was the bug.
fn classify_never_landed(
    submitted_slot: u64,
    blockhash_fetched_at_slot: u64,
    last_observed_slot: u64,
    tip_lamports: u64,
    tip_p50_at_submit: Option<u64>,
    tip_p75_at_submit: Option<u64>,
    jito_inflight: JitoInflight,
) -> Classification {
    // Genuine expiry: the hash was ALREADY past validity when we submitted.
    let age_at_submit = submitted_slot.saturating_sub(blockhash_fetched_at_slot);
    let expired_at_submission = age_at_submit > BLOCKHASH_VALIDITY_SLOTS;

    // Downstream symptom: aged past validity only while waiting to land.
    let age_at_last = last_observed_slot.saturating_sub(blockhash_fetched_at_slot);
    let aged_while_waiting = age_at_last > BLOCKHASH_VALIDITY_SLOTS;

    // "Outbid": the tip was strictly below the median (p50) at submit time.
    let tip_below_p50 = tip_p50_at_submit
        .map(|p50| tip_lamports < p50)
        .unwrap_or(false);

    let tip_ctx = tip_context(tip_lamports, tip_p50_at_submit, tip_p75_at_submit);
    let aged_symptom = if aged_while_waiting {
        format!(
            " (its blockhash later aged to {age_at_last} slots, past ~{BLOCKHASH_VALIDITY_SLOTS} \
             — a downstream symptom of sitting unlanded, not the cause)"
        )
    } else {
        String::new()
    };

    // 1. Blockhash stale AT submission -> genuine ExpiredBlockhash.
    if expired_at_submission {
        return Classification {
            kind: FailureKind::ExpiredBlockhash,
            confidence: Confidence::Certain,
            rationale: format!(
                "never landed: blockhash was ALREADY {age_at_submit} slots old at submission \
                 (fetched slot {blockhash_fetched_at_slot}, submitted slot {submitted_slot}, \
                 validity ~{BLOCKHASH_VALIDITY_SLOTS}) — stale before it ever reached the auction; \
                 {tip_ctx}"
            ),
        };
    }

    // 2. Sub-market tip -> outbid (actionable: raise the tip).
    if tip_below_p50 {
        let p50 = tip_p50_at_submit.expect("tip_below_p50 implies p50 is Some");
        return Classification {
            kind: FailureKind::FeeTooLow,
            confidence: Confidence::Likely,
            rationale: format!(
                "never landed: {tip_ctx} — tip below p50 {p50} (under market) is the likely root \
                 cause (outbid in the auction){aged_symptom}"
            ),
        };
    }

    // 3 & 4. Lost the auction: either Jito confirms it (Invalid/Failed) or we infer
    // it (competitive tip + valid-at-submit blockhash, yet it never landed).
    let jito_ctx = "block engine accepted the bundle (a bundle_id was returned)";
    match jito_inflight {
        JitoInflight::Invalid => Classification {
            kind: FailureKind::AuctionLost,
            confidence: Confidence::Certain,
            rationale: format!(
                "never landed: {jito_ctx} but getInflightBundleStatuses returned Invalid — the \
                 bundle is not in Jito's system / never entered its auction; it did not win. \
                 {tip_ctx}{aged_symptom}"
            ),
        },
        JitoInflight::Failed => Classification {
            kind: FailureKind::AuctionLost,
            confidence: Confidence::Likely,
            rationale: format!(
                "never landed: {jito_ctx} but getInflightBundleStatuses returned Failed (all \
                 regions failed/expired or it landed elsewhere) — it lost its auction. \
                 {tip_ctx}{aged_symptom}"
            ),
        },
        // No definitive Invalid/Failed signal: infer auction loss from the fact
        // that a well-formed bundle (valid-at-submit blockhash, competitive tip)
        // still never landed.
        JitoInflight::Pending | JitoInflight::Landed | JitoInflight::Unknown | JitoInflight::NotPolled => {
            Classification {
                kind: FailureKind::AuctionLost,
                confidence: Confidence::Ambiguous {
                    alternatives: vec![FailureKind::BundleFailure],
                },
                rationale: format!(
                    "never landed though the blockhash was valid at submission and the tip was \
                     competitive: {tip_ctx} — most likely lost the auction (or a skipped/dropped \
                     Jito leader slot); getInflightBundleStatuses was not a definitive \
                     Invalid/Failed when last polled{aged_symptom}"
                ),
            }
        }
    }
}

/// Describe the tip relative to the p50/p75 references for the rationale.
fn tip_context(tip: u64, p50: Option<u64>, p75: Option<u64>) -> String {
    match (p50, p75) {
        (Some(p50), Some(p75)) => format!("tip {tip} lamports (p50 {p50}, p75 {p75} at submit)"),
        (Some(p50), None) => format!("tip {tip} lamports (p50 {p50} at submit)"),
        (None, _) => format!("tip {tip} lamports (no tip floor reference at submit)"),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Whether a (lowercased) submit-error string indicates a transport / decode
/// failure *before* the auction — as opposed to an auction-level rejection.
/// Covers reqwest/serde decode failures (empty body), generic transport errors,
/// and the usual connection/timeout/DNS patterns.
fn looks_like_transport(lower: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "error decoding response body",
        "eof while parsing",
        "transport error",
        "request error",
        "error sending request",
        "connection refused",
        "connection reset",
        "connection closed",
        "connection error",
        "failed to connect",
        "error connecting",
        "timed out",
        "timeout",
        "dns error",
        "could not resolve",
        "tcp connect error",
        // Rate limiting / congestion: a pre-auction infrastructure failure, not
        // anything wrong with the bundle itself. Includes the HTTP 429 status and
        // Jito's JSON-RPC code -32097, so the 429 is caught no matter which call
        // produced it or how the message is phrased.
        "rate limited",
        "network congested",
        "globally rate limited",
        "too many requests",
        "http 429",
        "-32097",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

/// Decode a hex string (with optional `0x` prefix / surrounding whitespace) to
/// bytes. Returns `None` on odd length or any non-hex digit — never panics.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if s.is_empty() || !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize a real `TransactionError` exactly as the validator/stream does
    /// (bincode) and hex-encode it — proving the classifier round-trips against
    /// the genuine type.
    fn hex_of(err: &TransactionError) -> String {
        let bytes = bincode::serialize(err).expect("serialize TransactionError");
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    // --- 1. SubmitRejection ---

    #[test]
    fn submit_tip_too_low_is_fee_certain() {
        let c = classify(&Evidence::SubmitRejection {
            raw_error: "Bundle must tip at least 1000 lamports".to_string(),
        });
        assert_eq!(c.kind, FailureKind::FeeTooLow);
        assert_eq!(c.confidence, Confidence::Certain);
    }

    #[test]
    fn submit_tip_too_low_phrasing_variant() {
        let c = classify(&Evidence::SubmitRejection {
            raw_error: "tip is below the minimum required".to_string(),
        });
        assert_eq!(c.kind, FailureKind::FeeTooLow);
        assert_eq!(c.confidence, Confidence::Certain);
    }

    #[test]
    fn submit_simulation_failure_is_bundle_certain() {
        let c = classify(&Evidence::SubmitRejection {
            raw_error: "Bundle simulation failure: account not found".to_string(),
        });
        assert_eq!(c.kind, FailureKind::BundleFailure);
        assert_eq!(c.confidence, Confidence::Certain);
    }

    #[test]
    fn submit_malformed_is_bundle_certain() {
        let c = classify(&Evidence::SubmitRejection {
            raw_error: "failed to deserialize bundle transaction".to_string(),
        });
        assert_eq!(c.kind, FailureKind::BundleFailure);
        assert_eq!(c.confidence, Confidence::Certain);
    }

    #[test]
    fn submit_transport_decode_failure_is_transport_certain() {
        // The exact string observed on mainnet from the failing tip-account fetch.
        let raw = "transport error: random_tip_account: error decoding response body: \
                   EOF while parsing a value at line 1 column 0";
        let c = classify(&Evidence::SubmitRejection {
            raw_error: raw.to_string(),
        });
        assert_eq!(c.kind, FailureKind::TransportError);
        assert_eq!(c.confidence, Confidence::Certain);
        assert!(c.rationale.contains("never reached the auction"));
    }

    #[test]
    fn submit_transport_patterns_take_precedence() {
        // Connection/timeout patterns -> TransportError, not BundleFailure.
        for raw in [
            "transport error: send_bundle: error sending request for url (https://x): connection refused",
            "send_bundle: operation timed out",
            "transport error: dns error: failed to lookup address",
        ] {
            let c = classify(&Evidence::SubmitRejection {
                raw_error: raw.to_string(),
            });
            assert_eq!(c.kind, FailureKind::TransportError, "for {raw:?}");
            assert_eq!(c.confidence, Confidence::Certain);
        }
    }

    #[test]
    fn submit_rate_limit_congestion_is_transport_certain() {
        // The exact Block Engine string observed on a live run.
        let c = classify(&Evidence::SubmitRejection {
            raw_error: "bundle rejected by block engine: Network congested. Endpoint is globally rate limited."
                .to_string(),
        });
        assert_eq!(c.kind, FailureKind::TransportError);
        assert_eq!(c.confidence, Confidence::Certain);

        // Related rate-limit phrasings, too.
        for raw in [
            "bundle rejected by block engine: too many requests",
            "bundle rejected by block engine: rate limited, retry later",
        ] {
            let c = classify(&Evidence::SubmitRejection {
                raw_error: raw.to_string(),
            });
            assert_eq!(c.kind, FailureKind::TransportError, "for {raw:?}");
            assert_eq!(c.confidence, Confidence::Certain);
        }
    }

    #[test]
    fn send_bundle_429_is_transport() {
        // The exact error the submitter's send_bundle path produces on a 429 — the
        // SubmitError::Transport string wrapping the raw block-engine 429 body.
        // Must classify as TransportError (hold + resubmit), NOT a tip/bundle issue.
        let raw = r#"send_bundle: sendBundle HTTP 429 (globally rate limited): {"jsonrpc":"2.0","error":{"code":-32097,"message":"Network congested. Endpoint is globally rate limited."},"id":1}"#;
        let c = classify(&Evidence::SubmitRejection {
            raw_error: raw.to_string(),
        });
        assert_eq!(c.kind, FailureKind::TransportError);
        assert_eq!(c.confidence, Confidence::Certain);

        // The JSON-RPC code alone (any phrasing) is enough to catch it.
        let by_code = classify(&Evidence::SubmitRejection {
            raw_error: "send_bundle: server returned error -32097".to_string(),
        });
        assert_eq!(by_code.kind, FailureKind::TransportError);
    }

    #[test]
    fn genuine_auction_rejections_still_classify_normally() {
        // A real tip-too-low rejection must NOT be swallowed by the transport check.
        let fee = classify(&Evidence::SubmitRejection {
            raw_error: "bundle rejected by block engine: must tip at least 1000 lamports"
                .to_string(),
        });
        assert_eq!(fee.kind, FailureKind::FeeTooLow);
        let sim = classify(&Evidence::SubmitRejection {
            raw_error: "bundle rejected by block engine: simulation failure".to_string(),
        });
        assert_eq!(sim.kind, FailureKind::BundleFailure);
    }

    #[test]
    fn submit_unknown_is_bundle_likely_with_raw_preserved() {
        let raw = "some entirely novel block-engine error 0xdeadbeef";
        let c = classify(&Evidence::SubmitRejection {
            raw_error: raw.to_string(),
        });
        assert_eq!(c.kind, FailureKind::BundleFailure);
        assert_eq!(c.confidence, Confidence::Likely);
        assert!(c.rationale.contains(raw), "raw string must be preserved");
    }

    // --- 2. OnChainError (real bincode fixtures) ---

    #[test]
    fn on_chain_blockhash_not_found_is_expired_certain() {
        let hex = hex_of(&TransactionError::BlockhashNotFound);
        let c = classify(&Evidence::OnChainError {
            raw_error_hex: hex,
            slot: 1000,
        });
        assert_eq!(c.kind, FailureKind::ExpiredBlockhash);
        assert_eq!(c.confidence, Confidence::Certain);
        assert!(c.rationale.contains("1000"));
    }

    #[test]
    fn on_chain_computational_budget_exceeded_is_compute_certain() {
        let err =
            TransactionError::InstructionError(2, InstructionError::ComputationalBudgetExceeded);
        let c = classify(&Evidence::OnChainError {
            raw_error_hex: hex_of(&err),
            slot: 42,
        });
        assert_eq!(c.kind, FailureKind::ComputeExceeded);
        assert_eq!(c.confidence, Confidence::Certain);
        assert!(c.rationale.contains("instruction 2"));
    }

    #[test]
    fn on_chain_max_loaded_accounts_data_size_is_compute_certain() {
        let c = classify(&Evidence::OnChainError {
            raw_error_hex: hex_of(&TransactionError::MaxLoadedAccountsDataSizeExceeded),
            slot: 7,
        });
        assert_eq!(c.kind, FailureKind::ComputeExceeded);
        assert_eq!(c.confidence, Confidence::Certain);
    }

    #[test]
    fn on_chain_other_error_is_bundle_likely_with_debug() {
        let c = classify(&Evidence::OnChainError {
            raw_error_hex: hex_of(&TransactionError::AccountNotFound),
            slot: 9,
        });
        assert_eq!(c.kind, FailureKind::BundleFailure);
        assert_eq!(c.confidence, Confidence::Likely);
        assert!(c.rationale.contains("AccountNotFound"));
    }

    #[test]
    fn on_chain_0x_prefixed_hex_decodes() {
        let hex = format!("0x{}", hex_of(&TransactionError::BlockhashNotFound));
        let c = classify(&Evidence::OnChainError {
            raw_error_hex: hex,
            slot: 1,
        });
        assert_eq!(c.kind, FailureKind::ExpiredBlockhash);
    }

    #[test]
    fn on_chain_hex_garbage_is_ambiguous_not_panic() {
        let c = classify(&Evidence::OnChainError {
            raw_error_hex: "zzzz".to_string(), // not hex
            slot: 5,
        });
        assert_eq!(c.kind, FailureKind::BundleFailure);
        assert!(matches!(c.confidence, Confidence::Ambiguous { .. }));
        assert!(c.rationale.contains("zzzz"));
    }

    #[test]
    fn on_chain_valid_hex_but_not_a_tx_error_is_ambiguous() {
        // Valid hex, but a huge bogus enum discriminant -> bincode fails.
        let c = classify(&Evidence::OnChainError {
            raw_error_hex: "ffffffffffffffff".to_string(),
            slot: 5,
        });
        assert_eq!(c.kind, FailureKind::BundleFailure);
        assert!(matches!(c.confidence, Confidence::Ambiguous { .. }));
    }

    // --- 3. NeverLanded ---

    /// Blockhash was FRESH at submission (submitted_slot == fetched), so any age
    /// accrues only while waiting — i.e. a downstream symptom, never genuine
    /// ExpiredBlockhash. Jito status defaults to NotPolled.
    fn never_landed(
        blockhash_fetched_at_slot: u64,
        last_observed_slot: u64,
        tip: u64,
        p50: Option<u64>,
        p75: Option<u64>,
    ) -> Evidence {
        never_landed_full(
            blockhash_fetched_at_slot,
            blockhash_fetched_at_slot,
            last_observed_slot,
            tip,
            p50,
            p75,
            JitoInflight::NotPolled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn never_landed_full(
        submitted_slot: u64,
        blockhash_fetched_at_slot: u64,
        last_observed_slot: u64,
        tip: u64,
        p50: Option<u64>,
        p75: Option<u64>,
        jito_inflight: JitoInflight,
    ) -> Evidence {
        Evidence::NeverLanded {
            submitted_slot,
            blockhash_fetched_at_slot,
            last_observed_slot,
            tip_lamports: tip,
            tip_p50_at_submit: p50,
            tip_p75_at_submit: p75,
            jito_inflight,
        }
    }

    #[test]
    fn never_landed_invalid_status_competitive_tip_is_auction_lost() {
        // THE FIX: never-landed + Jito getInflightBundleStatuses=Invalid +
        // competitive tip (6000 >= p50 5000) + blockhash that only aged while
        // waiting -> AuctionLost (Certain), NOT ExpiredBlockhash.
        let c = classify(&never_landed_full(
            1000,
            1000,
            1200,
            6_000,
            Some(5_000),
            Some(5_500),
            JitoInflight::Invalid,
        ));
        assert_eq!(c.kind, FailureKind::AuctionLost);
        assert_eq!(c.confidence, Confidence::Certain);
        assert_ne!(c.kind, FailureKind::ExpiredBlockhash);
        assert!(c.rationale.contains("Invalid"));
        assert!(c.rationale.contains("bundle_id"));
        // The aged blockhash is named as a downstream symptom, not the cause.
        assert!(c.rationale.to_lowercase().contains("symptom"));
    }

    #[test]
    fn never_landed_failed_status_is_auction_lost_likely() {
        let c = classify(&never_landed_full(
            0,
            0,
            50,
            6_000,
            Some(5_000),
            None,
            JitoInflight::Failed,
        ));
        assert_eq!(c.kind, FailureKind::AuctionLost);
        assert_eq!(c.confidence, Confidence::Likely);
        assert!(c.rationale.contains("Failed"));
    }

    #[test]
    fn never_landed_expired_at_submission_is_expired_blockhash() {
        // Blockhash was ALREADY 200 slots old at submission (submitted 200, fetched
        // 0) -> genuine ExpiredBlockhash / Certain, even with Invalid status.
        let c = classify(&never_landed_full(
            200,
            0,
            260,
            6_000,
            Some(5_000),
            None,
            JitoInflight::Invalid,
        ));
        assert_eq!(c.kind, FailureKind::ExpiredBlockhash);
        assert_eq!(c.confidence, Confidence::Certain);
        assert!(c.rationale.contains("ALREADY"));
        assert!(c.rationale.contains("before it ever reached the auction"));
    }

    #[test]
    fn never_landed_competitive_tip_aged_while_waiting_is_auction_lost() {
        // Tip competitive (6000 >= p50 5000), blockhash valid at submit but aged to
        // 200 while waiting. This is the live-run bug: it used to be ExpiredBlockhash,
        // now AuctionLost (the expiry is a downstream symptom of never landing).
        let c = classify(&never_landed(0, 200, 6_000, Some(5_000), Some(5_500)));
        assert_eq!(c.kind, FailureKind::AuctionLost);
        assert_ne!(c.kind, FailureKind::ExpiredBlockhash);
        assert_eq!(
            c.confidence,
            Confidence::Ambiguous {
                alternatives: vec![FailureKind::BundleFailure]
            }
        );
        assert!(c.rationale.to_lowercase().contains("symptom"));
    }

    #[test]
    fn never_landed_fee_only() {
        // age = 50 (< 150), tip 100 < p50 1000 -> outbid -> FeeTooLow.
        let c = classify(&never_landed(1000, 1050, 100, Some(1000), Some(2000)));
        assert_eq!(c.kind, FailureKind::FeeTooLow);
        assert_eq!(c.confidence, Confidence::Likely);
        assert!(c.rationale.contains("tip 100"));
        assert!(c.rationale.contains("p50 1000"));
    }

    #[test]
    fn never_landed_sub_p50_tip_aged_while_waiting_blames_fee_not_expiry() {
        // Sub-p50 tip (900 < 1000) AND blockhash aged to 400 while waiting: the
        // sub-market tip is the actionable root cause -> FeeTooLow / Likely, with
        // the expiry named only as a downstream symptom (NOT ExpiredBlockhash).
        let c = classify(&never_landed(0, 400, 900, Some(1000), None));
        assert_eq!(c.kind, FailureKind::FeeTooLow);
        assert_eq!(c.confidence, Confidence::Likely);
        assert!(c.rationale.contains("root cause"));
        assert!(c.rationale.to_lowercase().contains("symptom"));
    }

    #[test]
    fn never_landed_sub_p50_tip_fresh_blockhash_is_fee_likely() {
        // Sub-p50 tip, blockhash still fresh -> clean FeeTooLow / Likely.
        let c = classify(&never_landed(0, 50, 1_511, Some(5_000), Some(12_000)));
        assert_eq!(c.kind, FailureKind::FeeTooLow);
        assert_eq!(c.confidence, Confidence::Likely);
        assert!(c.rationale.contains("1511"));
        assert!(c.rationale.contains("5000"));
    }

    #[test]
    fn never_landed_competitive_tip_fresh_blockhash_is_auction_lost() {
        // age = 100 (< 150), tip 5000 >= p50 5000, no Invalid signal -> inferred
        // AuctionLost (with BundleFailure as the alternative — skipped/dropped slot).
        let c = classify(&never_landed(900, 1000, 5000, Some(5000), Some(7000)));
        assert_eq!(c.kind, FailureKind::AuctionLost);
        assert_eq!(
            c.confidence,
            Confidence::Ambiguous {
                alternatives: vec![FailureKind::BundleFailure]
            }
        );
        assert!(c.rationale.to_lowercase().contains("leader"));
    }

    #[test]
    fn never_landed_no_tip_reference_is_auction_lost_not_expiry() {
        // No p50 -> fee never "in play". Blockhash fresh at submission in both
        // cases, so neither is ExpiredBlockhash -> AuctionLost.
        let c = classify(&never_landed(0, 200, 1, None, None));
        assert_eq!(c.kind, FailureKind::AuctionLost);
        let c2 = classify(&never_landed(0, 10, 1, None, None));
        assert_eq!(c2.kind, FailureKind::AuctionLost);
    }

    // --- boundary: aged-while-waiting no longer flips the KIND to ExpiredBlockhash ---

    #[test]
    fn aged_while_waiting_never_becomes_expired_blockhash() {
        // Tip competitive; only the (waiting) age moves across the 150 threshold.
        // The KIND stays AuctionLost throughout — expiry-while-waiting is a symptom.
        let competitive = (1000u64, Some(1000u64));
        for age in [149u64, 150, 151, 300] {
            let c = classify(&never_landed(0, age, competitive.0, competitive.1, None));
            assert_eq!(
                c.kind,
                FailureKind::AuctionLost,
                "age {age} should be AuctionLost, never ExpiredBlockhash"
            );
        }
    }

    #[test]
    fn tip_exactly_at_p50_is_not_fee_in_play() {
        // tip == p50 -> not "below", so fee is not in play; blockhash valid at
        // submit -> AuctionLost (inferred).
        let c = classify(&never_landed(0, 10, 1000, Some(1000), None));
        assert_eq!(c.kind, FailureKind::AuctionLost);
        // One lamport below -> fee in play.
        let c2 = classify(&never_landed(0, 10, 999, Some(1000), None));
        assert_eq!(c2.kind, FailureKind::FeeTooLow);
    }
}
