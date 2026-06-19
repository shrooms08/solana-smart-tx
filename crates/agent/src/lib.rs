//! AI decision layer for failure reasoning.
//!
//! The control loop hands a [`DecisionContext`] (a classified failure plus the
//! live slot/tip facts) to a [`DecisionAgent`] and gets back a [`Decision`] — an
//! ordered list of [`Action`]s plus a rationale. Two implementations:
//!
//! * [`BaselineAgent`] — deterministic, rule-based, no I/O. The safe fallback.
//! * [`FailureReasoningAgent`] — calls Claude (Anthropic Messages API) to reason
//!   over the evidence. On any failure (HTTP, timeout, schema-invalid reply) it
//!   returns `Err` rather than silently substituting — so the caller's fallback
//!   to the baseline is *observable*.
//!
//! Guardrails (tip clamping) live in [`clamp_actions`] so they're tested here,
//! and every decision is persisted via [`AgentLog`].

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Default model id. Overridable via [`AgentConfig::model`].
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-5";
const MAX_TOKENS: u32 = 1024;

/// The system prompt sent to the model. Asserted verbatim by a snapshot test so
/// any change is deliberate.
const SYSTEM_PROMPT: &str = r#"You are the failure-reasoning component inside a Solana Jito bundle-submission stack. A bundle just failed to land, and you must choose the corrective actions for the next attempt.

Domain facts:
- A transaction's recent blockhash is valid for only ~150 slots (~60 seconds). Once it is older than that the transaction can never land and the blockhash must be refreshed.
- Jito bundles win inclusion through a tip auction. Recently landed tips are tracked as rolling percentiles (p50, p75) in lamports; to beat the competition you generally must tip at or above the prevailing percentile.
- A Jito bundle is atomic: all of its transactions land together in a single leader's slot, or none of them do.
- Bundles can only land during slots led by Jito-enabled validators. If such a leader slot is skipped or missed, a perfectly valid, well-tipped bundle simply never lands and the right move may be to wait.
- Some failures happen BEFORE the bundle ever reaches the auction: a tip-account fetch failure, a sendBundle network/transport error, or an empty/malformed Block Engine response. These are classified as TransportError. When the failure kind is TransportError, the bundle, its blockhash, and its tip are NOT the cause — the infrastructure was unreachable. The only correct responses are to hold (wait for the infrastructure to recover) and then resubmit, or to abandon after repeated failures. Do NOT raise the tip and do NOT refresh the blockhash for a TransportError — nothing about the bundle caused it.
- AuctionLost means the Block Engine ACCEPTED the bundle (it returned a bundle_id) but the bundle never won its auction / never landed — often confirmed by Jito's getInflightBundleStatuses returning Invalid. The bundle was well-formed; it simply lost. By the time this is detected the blockhash IS stale (it aged while the bundle sat unlanded — a downstream symptom, not the original cause). The right move is usually to refresh the blockhash and resubmit to compete for the next Jito leader, optionally raising the tip if you were near or below the prevailing percentile; abandon after repeated losses. Do NOT describe this as an expired blockhash — the root cause is the lost auction.

Allowed actions:
- refresh_blockhash: fetch a fresh blockhash and rebuild (the blockhash is expired or aging).
- set_tip: raise the Jito tip to N lamports (the previous tip was uncompetitive).
- resubmit: send the bundle again unchanged.
- hold: wait N slots before retrying (wait out congestion, a Jito leader slot, or transport-layer recovery).
- abandon: give up on this transaction (retrying cannot help, e.g. the program itself errored or the compute budget was exceeded).

Reply with JSON ONLY. No prose, no markdown, no code fences. The reply MUST match this schema exactly:
{"actions":[{"type":"refresh_blockhash"}|{"type":"set_tip","lamports":N}|{"type":"resubmit"}|{"type":"hold","slots":N}|{"type":"abandon"}],"rationale":"..."}

List the actions in the order they should be applied. The "rationale" must cite the specific numbers from the context that drove your choice — the blockhash age in slots, the tip in lamports versus the current p50/p75, the attempt number, and the classified failure kind and confidence."#;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A prior attempt's outcome, for context on retries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorAttempt {
    pub attempt: u32,
    pub decision_summary: String,
    pub outcome: String,
}

/// Everything the decision layer needs to choose corrective actions.
///
/// Not `Serialize` directly (it embeds `tips::TipTrend`, which is not
/// serializable); use [`DecisionContext::to_json`] for the JSON view.
#[derive(Debug, Clone)]
pub struct DecisionContext {
    pub bundle_db_id: i64,
    pub classification: failure::Classification,
    pub evidence: failure::Evidence,
    pub blockhash_age_slots: u64,
    pub tip_lamports: u64,
    pub tip_p50_now: Option<u64>,
    pub tip_p75_now: Option<u64>,
    pub tip_trend: Option<tips::TipTrend>,
    pub tip_data_age_secs: Option<u64>,
    /// 1-based attempt number.
    pub attempt: u32,
    pub prior_attempts: Vec<PriorAttempt>,
    pub current_slot: u64,
}

impl DecisionContext {
    /// Serialize to readable (pretty) JSON — used both as the LLM user message
    /// and as the persisted `context_json`.
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(&self.wire())?)
    }

    fn wire(&self) -> ContextWire<'_> {
        ContextWire {
            bundle_db_id: self.bundle_db_id,
            classification: &self.classification,
            evidence: &self.evidence,
            blockhash_age_slots: self.blockhash_age_slots,
            tip_lamports: self.tip_lamports,
            tip_p50_now: self.tip_p50_now,
            tip_p75_now: self.tip_p75_now,
            tip_trend: self.tip_trend.map(|t| TipTrendWire {
                p50_change_lamports: t.p50_change_lamports,
                p75_change_lamports: t.p75_change_lamports,
                rising: t.rising,
            }),
            tip_data_age_secs: self.tip_data_age_secs,
            attempt: self.attempt,
            prior_attempts: &self.prior_attempts,
            current_slot: self.current_slot,
        }
    }
}

/// Serializable mirror of [`DecisionContext`] (works around `TipTrend` not being
/// `Serialize`).
#[derive(Serialize)]
struct ContextWire<'a> {
    bundle_db_id: i64,
    classification: &'a failure::Classification,
    evidence: &'a failure::Evidence,
    blockhash_age_slots: u64,
    tip_lamports: u64,
    tip_p50_now: Option<u64>,
    tip_p75_now: Option<u64>,
    tip_trend: Option<TipTrendWire>,
    tip_data_age_secs: Option<u64>,
    attempt: u32,
    prior_attempts: &'a [PriorAttempt],
    current_slot: u64,
}

#[derive(Serialize)]
struct TipTrendWire {
    p50_change_lamports: i64,
    p75_change_lamports: i64,
    rising: bool,
}

/// A corrective action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Fetch a fresh blockhash and rebuild.
    RefreshBlockhash,
    /// Raise the Jito tip to the given lamports.
    SetTip(u64),
    /// Resubmit unchanged.
    Resubmit,
    /// Wait `slots` slots before retrying.
    Hold { slots: u64 },
    /// Give up on this transaction.
    Abandon,
}

/// An agent's chosen actions plus its reasoning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub actions: Vec<Action>,
    pub rationale: String,
}

/// Strategy interface for the control loop. Async now — the LLM variant makes a
/// network call.
// `async fn` in a public trait triggers `async_fn_in_trait` (it can't express a
// `Send` bound on the returned future); that's fine here — callers `.await` the
// concrete agent rather than spawning an unbounded `dyn` future.
#[allow(async_fn_in_trait)]
pub trait DecisionAgent {
    async fn decide(&self, ctx: &DecisionContext) -> anyhow::Result<Decision>;
}

// ---------------------------------------------------------------------------
// Wire schema for actions (shared by the LLM reply parser and persistence)
// ---------------------------------------------------------------------------

/// The on-the-wire JSON form of an [`Action`] — `{"type":"set_tip","lamports":N}`
/// etc. Used to parse the model's reply AND to serialize `actions_json`, so the
/// persisted shape always matches the schema the model is told to emit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ActionWire {
    RefreshBlockhash,
    SetTip { lamports: u64 },
    Resubmit,
    Hold { slots: u64 },
    Abandon,
}

impl From<ActionWire> for Action {
    fn from(w: ActionWire) -> Self {
        match w {
            ActionWire::RefreshBlockhash => Action::RefreshBlockhash,
            ActionWire::SetTip { lamports } => Action::SetTip(lamports),
            ActionWire::Resubmit => Action::Resubmit,
            ActionWire::Hold { slots } => Action::Hold { slots },
            ActionWire::Abandon => Action::Abandon,
        }
    }
}

impl From<&Action> for ActionWire {
    fn from(a: &Action) -> Self {
        match a {
            Action::RefreshBlockhash => ActionWire::RefreshBlockhash,
            Action::SetTip(lamports) => ActionWire::SetTip { lamports: *lamports },
            Action::Resubmit => ActionWire::Resubmit,
            Action::Hold { slots } => ActionWire::Hold { slots: *slots },
            Action::Abandon => ActionWire::Abandon,
        }
    }
}

/// Serialize a list of actions to the canonical JSON array string.
pub fn actions_json(actions: &[Action]) -> String {
    let wire: Vec<ActionWire> = actions.iter().map(ActionWire::from).collect();
    serde_json::to_string(&wire).unwrap_or_else(|_| "[]".to_string())
}

// ---------------------------------------------------------------------------
// Guardrails
// ---------------------------------------------------------------------------

/// Clamp any `SetTip` action to `max_tip_lamports`. Returns the (possibly
/// rewritten) decision and whether anything was clamped. The orchestrator calls
/// this before executing an agent's decision.
pub fn clamp_actions(decision: &Decision, max_tip_lamports: u64) -> (Decision, bool) {
    let mut clamped = false;
    let actions = decision
        .actions
        .iter()
        .map(|action| match action {
            Action::SetTip(value) if *value > max_tip_lamports => {
                clamped = true;
                Action::SetTip(max_tip_lamports)
            }
            other => other.clone(),
        })
        .collect();
    (
        Decision {
            actions,
            rationale: decision.rationale.clone(),
        },
        clamped,
    )
}

// ---------------------------------------------------------------------------
// Baseline agent (deterministic)
// ---------------------------------------------------------------------------

/// Deterministic, rule-based agent. No I/O; the safe fallback when the LLM is
/// unavailable or returns garbage.
#[derive(Debug, Default, Clone)]
pub struct BaselineAgent;

impl BaselineAgent {
    /// The pure decision rule (sync) — also reused by [`DecisionAgent::decide`].
    pub fn decide_sync(&self, ctx: &DecisionContext) -> Decision {
        use failure::FailureKind::*;
        let kind = ctx.classification.kind;
        let actions = match kind {
            ExpiredBlockhash => vec![Action::RefreshBlockhash, Action::Resubmit],
            FeeTooLow => {
                let doubled = ctx.tip_lamports.saturating_mul(2);
                let target = ctx.tip_p75_now.map_or(doubled, |p75| p75.max(doubled));
                vec![
                    Action::RefreshBlockhash,
                    Action::SetTip(target),
                    Action::Resubmit,
                ]
            }
            ComputeExceeded => vec![Action::Abandon],
            BundleFailure => {
                if ctx.attempt <= 2 {
                    vec![Action::RefreshBlockhash, Action::Resubmit]
                } else {
                    vec![Action::Abandon]
                }
            }
            // Lost the auction (accepted but never won / Jito Invalid). The bundle
            // itself is fine; by timeout its blockhash IS stale, so refresh and
            // resubmit to compete for the next leader. Give up after a few tries.
            AuctionLost => {
                if ctx.attempt <= 3 {
                    vec![Action::RefreshBlockhash, Action::Resubmit]
                } else {
                    vec![Action::Abandon]
                }
            }
            // Transport failure: the bundle never reached the auction, so the
            // bundle/blockhash/tip are not the problem. Wait for infra to recover
            // and resubmit; give up after a few tries. No tip or blockhash change.
            TransportError => {
                if ctx.attempt <= 3 {
                    vec![Action::Hold { slots: 4 }, Action::Resubmit]
                } else {
                    vec![Action::Abandon]
                }
            }
        };
        Decision {
            actions,
            rationale: format!("{kind:?}"),
        }
    }
}

impl DecisionAgent for BaselineAgent {
    async fn decide(&self, ctx: &DecisionContext) -> anyhow::Result<Decision> {
        Ok(self.decide_sync(ctx))
    }
}

// ---------------------------------------------------------------------------
// HTTP seam (mockable)
// ---------------------------------------------------------------------------

/// A single HTTP POST request (the seam the LLM agent talks through).
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: serde_json::Value,
    pub timeout: Duration,
}

/// An HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// The HTTP transport seam. Mocked in tests; [`ReqwestTransport`] in production.
pub trait HttpTransport: Send + Sync {
    fn post(
        &self,
        request: HttpRequest,
    ) -> impl std::future::Future<Output = anyhow::Result<HttpResponse>> + Send;
}

/// Production transport backed by `reqwest`.
#[derive(Debug, Clone, Default)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl HttpTransport for ReqwestTransport {
    async fn post(&self, request: HttpRequest) -> anyhow::Result<HttpResponse> {
        let mut builder = self
            .client
            .post(&request.url)
            .timeout(request.timeout)
            .json(&request.body);
        for (name, value) in &request.headers {
            builder = builder.header(*name, value);
        }
        let response = builder.send().await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        Ok(HttpResponse { status, body })
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the LLM agent.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Anthropic API key (sent as the `x-api-key` header).
    pub api_key: String,
    /// Model id.
    pub model: String,
    /// Tip ceiling the orchestrator clamps to (see [`clamp_actions`]).
    pub max_tip_lamports: u64,
    /// Per-request timeout.
    pub request_timeout: Duration,
}

impl AgentConfig {
    /// Construct with the given API key and sensible defaults for the rest.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            ..Self::default()
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: DEFAULT_MODEL.to_string(),
            max_tip_lamports: 1_000_000, // 0.001 SOL
            request_timeout: Duration::from_secs(10),
        }
    }
}

// ---------------------------------------------------------------------------
// Failure-reasoning agent (LLM)
// ---------------------------------------------------------------------------

/// LLM-backed agent. Calls the Anthropic Messages API and parses a strict JSON
/// reply into a [`Decision`].
#[derive(Debug, Clone)]
pub struct FailureReasoningAgent<T = ReqwestTransport> {
    config: AgentConfig,
    transport: T,
}

impl FailureReasoningAgent<ReqwestTransport> {
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            transport: ReqwestTransport::new(),
        }
    }
}

impl<T: HttpTransport> FailureReasoningAgent<T> {
    /// Construct over an arbitrary transport (used by tests).
    pub fn with_transport(config: AgentConfig, transport: T) -> Self {
        Self { config, transport }
    }

    fn build_request(&self, ctx: &DecisionContext) -> anyhow::Result<HttpRequest> {
        let user_message = ctx.to_json().context("serializing decision context")?;
        let body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": MAX_TOKENS,
            "temperature": 0.0,
            "system": SYSTEM_PROMPT,
            "messages": [ { "role": "user", "content": user_message } ],
        });
        Ok(HttpRequest {
            url: ANTHROPIC_URL.to_string(),
            headers: vec![
                ("x-api-key", self.config.api_key.clone()),
                ("anthropic-version", ANTHROPIC_VERSION.to_string()),
            ],
            body,
            timeout: self.config.request_timeout,
        })
    }
}

impl<T: HttpTransport> DecisionAgent for FailureReasoningAgent<T> {
    async fn decide(&self, ctx: &DecisionContext) -> anyhow::Result<Decision> {
        let request = self.build_request(ctx)?;
        let response = self
            .transport
            .post(request)
            .await
            .context("anthropic request failed")?;

        if !(200..300).contains(&response.status) {
            // Never include request headers (the api key) — only the response.
            anyhow::bail!(
                "anthropic API returned HTTP {}: {}",
                response.status,
                response.body
            );
        }

        let text = extract_text(&response.body)
            .context("extracting text from anthropic response envelope")?;
        parse_reply(&text).context("parsing decision JSON from model reply")
    }
}

/// Anthropic Messages API success envelope (the parts we need).
#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

/// Pull the concatenated `text` blocks out of the response envelope.
fn extract_text(body: &str) -> anyhow::Result<String> {
    let parsed: AnthropicResponse =
        serde_json::from_str(body).with_context(|| format!("unexpected response body: {body}"))?;
    let text: String = parsed
        .content
        .into_iter()
        .filter(|b| b.kind == "text")
        .map(|b| b.text)
        .collect();
    if text.trim().is_empty() {
        anyhow::bail!("anthropic response contained no text content");
    }
    Ok(text)
}

/// The model's required reply schema.
#[derive(Deserialize)]
struct ReplyWire {
    actions: Vec<ActionWire>,
    rationale: String,
}

/// Strictly parse a model reply into a [`Decision`]. Tolerates surrounding code
/// fences/whitespace and unknown extra fields, but rejects anything that doesn't
/// satisfy the required schema.
fn parse_reply(text: &str) -> anyhow::Result<Decision> {
    let cleaned = strip_code_fences(text.trim());
    let reply: ReplyWire = serde_json::from_str(cleaned)
        .with_context(|| format!("model reply was not valid decision JSON: {cleaned}"))?;
    Ok(Decision {
        actions: reply.actions.into_iter().map(Action::from).collect(),
        rationale: reply.rationale,
    })
}

/// Strip a single ```/```json fenced block, if present.
fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else {
        return s;
    };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let rest = rest.trim_start_matches(['\n', '\r']);
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

/// The exact system prompt (for inspection / snapshot tests).
pub fn system_prompt() -> &'static str {
    SYSTEM_PROMPT
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Which agent produced a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Llm,
    Baseline,
}

impl AgentKind {
    fn as_str(self) -> &'static str {
        match self {
            AgentKind::Llm => "llm",
            AgentKind::Baseline => "baseline",
        }
    }
}

/// A decision to persist.
#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub bundle_db_id: i64,
    pub attempt: u32,
    pub agent_kind: AgentKind,
    pub context_json: String,
    pub actions_json: String,
    pub rationale: String,
    pub model: Option<String>,
    pub latency_ms: Option<u64>,
    pub executed: bool,
}

impl DecisionRecord {
    /// Build a record from a [`Decision`] (serializing its actions).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bundle_db_id: i64,
        attempt: u32,
        agent_kind: AgentKind,
        context_json: String,
        decision: &Decision,
        model: Option<String>,
        latency_ms: Option<u64>,
        executed: bool,
    ) -> Self {
        Self {
            bundle_db_id,
            attempt,
            agent_kind,
            context_json,
            actions_json: actions_json(&decision.actions),
            rationale: decision.rationale.clone(),
            model,
            latency_ms,
            executed,
        }
    }
}

/// Persistence handle for `agent_decisions`, over a shared SQLite pool.
#[derive(Debug, Clone)]
pub struct AgentLog {
    pool: SqlitePool,
}

impl AgentLog {
    /// Open over `pool`, ensuring the `agent_decisions` table exists.
    pub async fn new(pool: SqlitePool) -> anyhow::Result<Self> {
        sqlx::raw_sql(include_str!("../migrations/0001_agent_decisions.sql"))
            .execute(&pool)
            .await
            .context("creating agent_decisions table")?;
        Ok(Self { pool })
    }

    /// Persist a decision; returns the new row id.
    pub async fn record_decision(&self, record: &DecisionRecord) -> anyhow::Result<i64> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO agent_decisions (\
                bundle_db_id, attempt, agent_kind, context_json, actions_json, \
                rationale, model, latency_ms, executed, created_at) \
             VALUES (?,?,?,?,?,?,?,?,?,?) RETURNING id",
        )
        .bind(record.bundle_db_id)
        .bind(record.attempt as i64)
        .bind(record.agent_kind.as_str())
        .bind(&record.context_json)
        .bind(&record.actions_json)
        .bind(&record.rationale)
        .bind(&record.model)
        .bind(record.latency_ms.map(|v| v as i64))
        .bind(record.executed)
        .bind(now_millis())
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests (offline)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use failure::{Classification, Confidence, Evidence, FailureKind};

    fn classification(kind: FailureKind) -> Classification {
        Classification {
            kind,
            confidence: Confidence::Certain,
            rationale: "test".to_string(),
        }
    }

    fn ctx(kind: FailureKind, attempt: u32, tip: u64, p75: Option<u64>) -> DecisionContext {
        DecisionContext {
            bundle_db_id: 1,
            classification: classification(kind),
            evidence: Evidence::SubmitRejection {
                raw_error: "test".to_string(),
            },
            blockhash_age_slots: 42,
            tip_lamports: tip,
            tip_p50_now: Some(5_000),
            tip_p75_now: p75,
            tip_trend: Some(tips::TipTrend {
                p50_change_lamports: 100,
                p75_change_lamports: -50,
                rising: true,
            }),
            tip_data_age_secs: Some(3),
            attempt,
            prior_attempts: vec![],
            current_slot: 1000,
        }
    }

    // --- baseline: all four kinds ---

    #[tokio::test]
    async fn baseline_expired_blockhash() {
        let d = BaselineAgent
            .decide(&ctx(FailureKind::ExpiredBlockhash, 1, 10_000, None))
            .await
            .unwrap();
        assert_eq!(d.actions, vec![Action::RefreshBlockhash, Action::Resubmit]);
        assert_eq!(d.rationale, "ExpiredBlockhash");
    }

    #[test]
    fn baseline_fee_too_low_uses_max_of_p75_and_double() {
        // max(p75 50k, tip*2 20k) -> 50k.
        let d = BaselineAgent.decide_sync(&ctx(FailureKind::FeeTooLow, 1, 10_000, Some(50_000)));
        assert_eq!(
            d.actions,
            vec![
                Action::RefreshBlockhash,
                Action::SetTip(50_000),
                Action::Resubmit
            ]
        );

        // p75 below tip*2 -> double wins.
        let d2 = BaselineAgent.decide_sync(&ctx(FailureKind::FeeTooLow, 1, 10_000, Some(5_000)));
        assert_eq!(d2.actions[1], Action::SetTip(20_000));

        // No p75 -> double.
        let d3 = BaselineAgent.decide_sync(&ctx(FailureKind::FeeTooLow, 1, 7_000, None));
        assert_eq!(d3.actions[1], Action::SetTip(14_000));
    }

    #[test]
    fn baseline_compute_exceeded_abandons() {
        let d = BaselineAgent.decide_sync(&ctx(FailureKind::ComputeExceeded, 1, 10_000, None));
        assert_eq!(d.actions, vec![Action::Abandon]);
    }

    #[test]
    fn baseline_bundle_failure_retries_then_abandons() {
        let retry = BaselineAgent.decide_sync(&ctx(FailureKind::BundleFailure, 2, 10_000, None));
        assert_eq!(retry.actions, vec![Action::RefreshBlockhash, Action::Resubmit]);
        let give_up = BaselineAgent.decide_sync(&ctx(FailureKind::BundleFailure, 3, 10_000, None));
        assert_eq!(give_up.actions, vec![Action::Abandon]);
    }

    #[test]
    fn baseline_auction_lost_refreshes_blockhash_and_resubmits_then_abandons() {
        // Lost the auction: blockhash is stale by now, so refresh + resubmit to
        // compete for the next leader; abandon after repeated losses.
        let retry = BaselineAgent.decide_sync(&ctx(FailureKind::AuctionLost, 3, 10_000, None));
        assert_eq!(retry.actions, vec![Action::RefreshBlockhash, Action::Resubmit]);
        let give_up = BaselineAgent.decide_sync(&ctx(FailureKind::AuctionLost, 4, 10_000, None));
        assert_eq!(give_up.actions, vec![Action::Abandon]);
    }

    #[test]
    fn baseline_transport_error_holds_then_resubmits_no_tip_change() {
        // Hold + resubmit while attempts <= 3; no tip change, no blockhash refresh.
        let early = BaselineAgent.decide_sync(&ctx(FailureKind::TransportError, 3, 10_000, Some(99)));
        assert_eq!(early.actions, vec![Action::Hold { slots: 4 }, Action::Resubmit]);
        assert!(!early
            .actions
            .iter()
            .any(|a| matches!(a, Action::SetTip(_) | Action::RefreshBlockhash)));
        // Abandon after repeated failures.
        let late = BaselineAgent.decide_sync(&ctx(FailureKind::TransportError, 4, 10_000, None));
        assert_eq!(late.actions, vec![Action::Abandon]);
    }

    // --- JSON reply parsing ---

    #[test]
    fn parse_valid_reply() {
        let json = r#"{"actions":[{"type":"refresh_blockhash"},{"type":"set_tip","lamports":50000},{"type":"resubmit"}],"rationale":"blockhash 163 slots old; tip 10000 below p75 50000"}"#;
        let d = parse_reply(json).unwrap();
        assert_eq!(
            d.actions,
            vec![
                Action::RefreshBlockhash,
                Action::SetTip(50_000),
                Action::Resubmit
            ]
        );
        assert!(d.rationale.contains("163 slots"));
    }

    #[test]
    fn parse_reply_with_hold_and_abandon() {
        let json = r#"{"actions":[{"type":"hold","slots":4},{"type":"abandon"}],"rationale":"x"}"#;
        let d = parse_reply(json).unwrap();
        assert_eq!(d.actions, vec![Action::Hold { slots: 4 }, Action::Abandon]);
    }

    #[test]
    fn parse_reply_tolerates_code_fences_and_extra_fields() {
        // Markdown fence + an unknown extra field on the action and at top level.
        let json = "```json\n{\"actions\":[{\"type\":\"resubmit\",\"note\":\"ignored\"}],\"rationale\":\"r\",\"confidence\":0.9}\n```";
        let d = parse_reply(json).unwrap();
        assert_eq!(d.actions, vec![Action::Resubmit]);
        assert_eq!(d.rationale, "r");
    }

    #[test]
    fn parse_invalid_replies_error() {
        // Not JSON.
        assert!(parse_reply("sorry, I can't do that").is_err());
        // Missing required `rationale`.
        assert!(parse_reply(r#"{"actions":[{"type":"resubmit"}]}"#).is_err());
        // Unknown action type.
        assert!(parse_reply(r#"{"actions":[{"type":"nuke"}],"rationale":"x"}"#).is_err());
        // set_tip missing its lamports field.
        assert!(parse_reply(r#"{"actions":[{"type":"set_tip"}],"rationale":"x"}"#).is_err());
    }

    // --- clamp ---

    #[test]
    fn clamp_caps_set_tip() {
        let decision = Decision {
            actions: vec![
                Action::RefreshBlockhash,
                Action::SetTip(5_000_000),
                Action::SetTip(100),
                Action::Resubmit,
            ],
            rationale: "r".to_string(),
        };
        let (clamped, was) = clamp_actions(&decision, 1_000_000);
        assert!(was);
        assert_eq!(
            clamped.actions,
            vec![
                Action::RefreshBlockhash,
                Action::SetTip(1_000_000), // capped
                Action::SetTip(100),       // untouched
                Action::Resubmit,
            ]
        );
    }

    #[test]
    fn clamp_noop_when_under_cap() {
        let decision = Decision {
            actions: vec![Action::SetTip(500), Action::Abandon],
            rationale: "r".to_string(),
        };
        let (out, was) = clamp_actions(&decision, 1_000_000);
        assert!(!was);
        assert_eq!(out.actions, decision.actions);
    }

    // --- system prompt snapshot (asserted verbatim so changes are deliberate) ---

    #[test]
    fn system_prompt_snapshot() {
        let expected = r#"You are the failure-reasoning component inside a Solana Jito bundle-submission stack. A bundle just failed to land, and you must choose the corrective actions for the next attempt.

Domain facts:
- A transaction's recent blockhash is valid for only ~150 slots (~60 seconds). Once it is older than that the transaction can never land and the blockhash must be refreshed.
- Jito bundles win inclusion through a tip auction. Recently landed tips are tracked as rolling percentiles (p50, p75) in lamports; to beat the competition you generally must tip at or above the prevailing percentile.
- A Jito bundle is atomic: all of its transactions land together in a single leader's slot, or none of them do.
- Bundles can only land during slots led by Jito-enabled validators. If such a leader slot is skipped or missed, a perfectly valid, well-tipped bundle simply never lands and the right move may be to wait.
- Some failures happen BEFORE the bundle ever reaches the auction: a tip-account fetch failure, a sendBundle network/transport error, or an empty/malformed Block Engine response. These are classified as TransportError. When the failure kind is TransportError, the bundle, its blockhash, and its tip are NOT the cause — the infrastructure was unreachable. The only correct responses are to hold (wait for the infrastructure to recover) and then resubmit, or to abandon after repeated failures. Do NOT raise the tip and do NOT refresh the blockhash for a TransportError — nothing about the bundle caused it.
- AuctionLost means the Block Engine ACCEPTED the bundle (it returned a bundle_id) but the bundle never won its auction / never landed — often confirmed by Jito's getInflightBundleStatuses returning Invalid. The bundle was well-formed; it simply lost. By the time this is detected the blockhash IS stale (it aged while the bundle sat unlanded — a downstream symptom, not the original cause). The right move is usually to refresh the blockhash and resubmit to compete for the next Jito leader, optionally raising the tip if you were near or below the prevailing percentile; abandon after repeated losses. Do NOT describe this as an expired blockhash — the root cause is the lost auction.

Allowed actions:
- refresh_blockhash: fetch a fresh blockhash and rebuild (the blockhash is expired or aging).
- set_tip: raise the Jito tip to N lamports (the previous tip was uncompetitive).
- resubmit: send the bundle again unchanged.
- hold: wait N slots before retrying (wait out congestion, a Jito leader slot, or transport-layer recovery).
- abandon: give up on this transaction (retrying cannot help, e.g. the program itself errored or the compute budget was exceeded).

Reply with JSON ONLY. No prose, no markdown, no code fences. The reply MUST match this schema exactly:
{"actions":[{"type":"refresh_blockhash"}|{"type":"set_tip","lamports":N}|{"type":"resubmit"}|{"type":"hold","slots":N}|{"type":"abandon"}],"rationale":"..."}

List the actions in the order they should be applied. The "rationale" must cite the specific numbers from the context that drove your choice — the blockhash age in slots, the tip in lamports versus the current p50/p75, the attempt number, and the classified failure kind and confidence."#;
        assert_eq!(system_prompt(), expected);
    }

    // --- context serialization ---

    #[test]
    fn context_serializes_to_json_with_trend() {
        let json = ctx(FailureKind::FeeTooLow, 2, 10_000, Some(50_000))
            .to_json()
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["tip_lamports"], 10_000);
        assert_eq!(value["attempt"], 2);
        assert_eq!(value["tip_trend"]["p50_change_lamports"], 100);
        assert_eq!(value["classification"]["kind"], "FeeTooLow");
    }

    // --- LLM agent over a mocked transport ---

    struct MockHttp {
        status: u16,
        body: String,
    }

    impl HttpTransport for MockHttp {
        async fn post(&self, _request: HttpRequest) -> anyhow::Result<HttpResponse> {
            Ok(HttpResponse {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }

    struct ErrHttp;
    impl HttpTransport for ErrHttp {
        async fn post(&self, _request: HttpRequest) -> anyhow::Result<HttpResponse> {
            anyhow::bail!("simulated transport failure (e.g. timeout)")
        }
    }

    fn envelope(text: &str) -> String {
        serde_json::json!({ "content": [ { "type": "text", "text": text } ] }).to_string()
    }

    #[tokio::test]
    async fn llm_agent_parses_success() {
        let reply = r#"{"actions":[{"type":"set_tip","lamports":40000},{"type":"resubmit"}],"rationale":"tip 10000 below p75 40000 at attempt 1"}"#;
        let agent = FailureReasoningAgent::with_transport(
            AgentConfig::new("sk-test"),
            MockHttp {
                status: 200,
                body: envelope(reply),
            },
        );
        let d = agent
            .decide(&ctx(FailureKind::FeeTooLow, 1, 10_000, Some(40_000)))
            .await
            .unwrap();
        assert_eq!(d.actions, vec![Action::SetTip(40_000), Action::Resubmit]);
        assert!(d.rationale.contains("p75 40000"));
    }

    #[tokio::test]
    async fn llm_agent_errors_on_http_error() {
        let agent = FailureReasoningAgent::with_transport(
            AgentConfig::new("sk-test"),
            MockHttp {
                status: 529,
                body: "overloaded".to_string(),
            },
        );
        let err = agent
            .decide(&ctx(FailureKind::BundleFailure, 1, 1, None))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("529"));
    }

    #[tokio::test]
    async fn llm_agent_errors_on_transport_failure() {
        let agent = FailureReasoningAgent::with_transport(AgentConfig::new("sk-test"), ErrHttp);
        assert!(agent
            .decide(&ctx(FailureKind::BundleFailure, 1, 1, None))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn llm_agent_errors_on_schema_invalid_reply() {
        let agent = FailureReasoningAgent::with_transport(
            AgentConfig::new("sk-test"),
            MockHttp {
                status: 200,
                body: envelope("not the json you are looking for"),
            },
        );
        assert!(agent
            .decide(&ctx(FailureKind::BundleFailure, 1, 1, None))
            .await
            .is_err());
    }

    // --- persistence ---

    async fn mem_pool() -> SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn agent_log_records_executed_and_shadow() {
        let log = AgentLog::new(mem_pool().await).await.unwrap();
        let decision = Decision {
            actions: vec![
                Action::RefreshBlockhash,
                Action::SetTip(50_000),
                Action::Resubmit,
            ],
            rationale: "llm reasoning".to_string(),
        };
        let context_json = ctx(FailureKind::FeeTooLow, 1, 10_000, Some(50_000))
            .to_json()
            .unwrap();

        // The executed LLM decision.
        let llm_id = log
            .record_decision(&DecisionRecord::new(
                7,
                1,
                AgentKind::Llm,
                context_json.clone(),
                &decision,
                Some("claude-sonnet-4-5".to_string()),
                Some(842),
                true,
            ))
            .await
            .unwrap();

        // The shadow baseline (executed = false).
        let baseline =
            BaselineAgent.decide_sync(&ctx(FailureKind::FeeTooLow, 1, 10_000, Some(50_000)));
        log.record_decision(&DecisionRecord::new(
            7,
            1,
            AgentKind::Baseline,
            context_json,
            &baseline,
            None,
            None,
            false,
        ))
        .await
        .unwrap();

        assert!(llm_id > 0);

        // Read the LLM row back.
        let (kind, executed, model, actions_json_db, rationale): (
            String,
            bool,
            Option<String>,
            String,
            String,
        ) = sqlx::query_as(
            "SELECT agent_kind, executed, model, actions_json, rationale \
                 FROM agent_decisions WHERE id = ?",
        )
        .bind(llm_id)
        .fetch_one(&log.pool)
        .await
        .unwrap();
        assert_eq!(kind, "llm");
        assert!(executed);
        assert_eq!(model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(rationale, "llm reasoning");
        // actions_json matches the canonical schema shape.
        assert_eq!(
            actions_json_db,
            r#"[{"type":"refresh_blockhash"},{"type":"set_tip","lamports":50000},{"type":"resubmit"}]"#
        );

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_decisions WHERE bundle_db_id = 7")
                .fetch_one(&log.pool)
                .await
                .unwrap();
        assert_eq!(count, 2);

        let shadow_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_decisions WHERE executed = 0")
                .fetch_one(&log.pool)
                .await
                .unwrap();
        assert_eq!(shadow_count, 1);
    }
}
