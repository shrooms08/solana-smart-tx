//! Failure classification for the smart-transaction stack.
//!
//! Turns raw error signals (RPC errors, dropped bundles, expired blockhashes,
//! simulation failures) into a small, decision-friendly taxonomy that the
//! `agent` crate reasons over.

use serde::{Deserialize, Serialize};

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
}

/// Opaque, source-agnostic signal handed to the classifier.
///
/// TODO: flesh this out with the real inputs (RPC error payloads, bundle
/// status responses, simulation logs, slot deltas, ...). For now it is a thin
/// placeholder so call sites and the classifier signature can be wired up.
#[derive(Debug, Default, Clone)]
pub struct FailureSignal {
    /// Raw error / status text, if any.
    pub message: Option<String>,
}

/// Classify a raw failure signal into a [`FailureKind`].
///
/// TODO: implement real classification (pattern-match on RPC error codes,
/// bundle status, sim logs, blockhash-age heuristics). Currently a stub.
pub fn classify(_signal: &FailureSignal) -> Option<FailureKind> {
    // TODO: real classification logic.
    None
}
