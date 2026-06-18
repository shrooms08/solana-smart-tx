-- Every decision the agent layer makes, persisted for audit and analysis.
--
-- The orchestrator writes two rows per failed-bundle attempt: the LLM decision
-- it actually executes (`agent_kind='llm'`, `executed=1`) and the deterministic
-- baseline as a shadow (`agent_kind='baseline'`, `executed=0`), so the two can
-- be compared after the fact.
--
-- Applied idempotently (CREATE ... IF NOT EXISTS) on the same SQLite pool the
-- lifecycle crate uses; it deliberately does NOT go through sqlx's Migrator /
-- _sqlx_migrations table, to avoid a migration-version collision with the
-- lifecycle migration (both would be version 0001).
CREATE TABLE IF NOT EXISTS agent_decisions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    -- FK (by value) to lifecycle's bundle_submissions.id.
    bundle_db_id  INTEGER NOT NULL,
    -- 1-based attempt number this decision was made for.
    attempt       INTEGER NOT NULL,
    -- 'llm' | 'baseline'.
    agent_kind    TEXT    NOT NULL,
    -- The DecisionContext serialized as JSON (the exact facts the agent saw).
    context_json  TEXT    NOT NULL,
    -- The chosen actions as JSON (same schema the LLM must emit).
    actions_json  TEXT    NOT NULL,
    -- The decision's rationale.
    rationale     TEXT    NOT NULL,
    -- Model id for LLM decisions; NULL for the baseline.
    model         TEXT,
    -- End-to-end LLM call latency (ms); NULL for the baseline (no network).
    latency_ms    INTEGER,
    -- Whether this decision was actually executed (vs. a shadow record).
    executed      INTEGER NOT NULL,
    -- Unix epoch milliseconds.
    created_at    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_decisions_bundle_db_id
    ON agent_decisions (bundle_db_id);
CREATE INDEX IF NOT EXISTS idx_agent_decisions_agent_kind
    ON agent_decisions (agent_kind);
