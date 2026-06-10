//! AI decision layer.
//!
//! The control loop feeds a [`DecisionContext`] to a [`DecisionAgent`] and gets
//! back a [`Decision`]. Two implementations are provided:
//!
//! * [`BaselineAgent`] — deterministic, rule-based (no I/O).
//! * [`FailureReasoningAgent`] — calls Claude to reason about the failure.
//!
//! Only the trait surface is stable right now; both impls are stubs.

use failure::FailureKind;

/// Everything the decision layer needs to choose a corrective action.
///
/// TODO: replace the placeholder fields with the real signal set once the
/// upstream crates expose concrete types (e.g. typed blockhash, tip stats).
#[derive(Debug, Clone)]
pub struct DecisionContext {
    /// Why the previous attempt failed, if it failed.
    pub failure_kind: Option<FailureKind>,
    /// Slot observed at decision time.
    pub slot: u64,
    /// How many slots old the working blockhash is.
    pub blockhash_age_slots: u64,
    /// Recent tip percentiles (lamports), e.g. [p25, p50, p75, p95].
    pub recent_tip_percentiles: Vec<u64>,
    /// How many times this transaction has been attempted so far.
    pub attempt_count: u32,
}

/// A corrective action chosen by a [`DecisionAgent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Fetch a fresh blockhash and rebuild.
    RefreshBlockhash,
    /// Raise the Jito tip to the given lamports.
    BumpTip(u64),
    /// Set the compute-unit limit to the given value.
    AdjustCompute(u32),
    /// Wait until the given slot before resubmitting.
    HoldUntilSlot(u64),
    /// Give up on this transaction.
    Abandon,
    /// Resubmit unchanged.
    Resubmit,
}

/// Strategy interface for the control loop.
pub trait DecisionAgent {
    /// Choose a [`Decision`] for the given context.
    fn decide(&self, ctx: DecisionContext) -> Decision;
}

/// Deterministic, rule-based agent (no network calls).
///
/// TODO: implement the deterministic policy (e.g. refresh on expired blockhash,
/// bump tip toward p75 on fee-too-low, abandon after N attempts).
#[derive(Debug, Default, Clone)]
pub struct BaselineAgent;

impl DecisionAgent for BaselineAgent {
    fn decide(&self, _ctx: DecisionContext) -> Decision {
        // TODO: deterministic decision rules.
        unimplemented!("BaselineAgent::decide is not implemented yet")
    }
}

/// LLM-backed agent that asks Claude to reason about the failure.
///
/// TODO: hold an HTTP client + API key/model config and call the Anthropic API
/// inside `decide` (or a future async variant), parsing the response into a
/// [`Decision`].
#[derive(Debug, Default, Clone)]
pub struct FailureReasoningAgent {
    // TODO: client, api_key, model, system prompt, etc.
}

impl DecisionAgent for FailureReasoningAgent {
    fn decide(&self, _ctx: DecisionContext) -> Decision {
        // TODO: build prompt from ctx, call Claude, parse Decision.
        unimplemented!("FailureReasoningAgent::decide is not implemented yet")
    }
}
