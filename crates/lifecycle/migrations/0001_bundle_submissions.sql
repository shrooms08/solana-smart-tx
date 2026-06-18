-- One row per Jito bundle submission, advanced through its commitment stages
-- (Submitted -> Processed -> Confirmed -> Finalized, or terminal Failed) by the
-- lifecycle tracker. This table is the judged deliverable: it captures the full
-- construction metadata, per-stage timestamps, inter-stage latency deltas, and
-- the failure classification when a bundle does not land.
--
-- All timestamps are unix epoch MILLISECONDS (INTEGER). Slots and lamports are
-- stored as INTEGER (SQLite i64).
CREATE TABLE IF NOT EXISTS bundle_submissions (
    id                        INTEGER PRIMARY KEY AUTOINCREMENT,

    -- Construction metadata (from submitter::BundleRecord).
    bundle_id                 TEXT    NOT NULL UNIQUE,
    memo_signature            TEXT    NOT NULL,
    tip_signature             TEXT    NOT NULL,
    tip_account               TEXT    NOT NULL,
    blockhash                 TEXT    NOT NULL,
    blockhash_fetched_at_slot INTEGER NOT NULL,
    submitted_slot            INTEGER NOT NULL,
    tip_lamports              INTEGER NOT NULL,
    fault_injected            TEXT,

    -- Tip-market context stamped at submit time, for later NeverLanded evidence.
    tip_p50_at_submit         INTEGER,
    tip_p75_at_submit         INTEGER,

    -- Lifecycle state: 'Submitted' | 'Processed' | 'Confirmed' | 'Finalized' | 'Failed'.
    status                    TEXT    NOT NULL,

    -- Per-stage timestamps (unix millis). NULL until the stage is reached.
    submitted_at              INTEGER NOT NULL,
    processed_at              INTEGER,
    confirmed_at              INTEGER,
    finalized_at              INTEGER,

    -- Slot the transaction landed in (set at Processed).
    landed_slot               INTEGER,

    -- Inter-stage latency deltas (millis), derived from the timestamps above.
    submit_to_process_ms      INTEGER,
    process_to_confirm_ms     INTEGER,
    confirm_to_finalize_ms    INTEGER,

    -- Terminal failure classification (failure::Classification), NULL otherwise.
    failure_kind              TEXT,
    failure_confidence        TEXT,
    failure_rationale         TEXT,

    -- Provenance of each transition: 'stream' (live) or 'reconcile' (catch-up
    -- via getSignatureStatuses after a stream reconnect). NULL until applied.
    processed_source          TEXT,
    confirmed_source          TEXT,
    finalized_source          TEXT
);

CREATE INDEX IF NOT EXISTS idx_bundle_submissions_status
    ON bundle_submissions (status);
CREATE INDEX IF NOT EXISTS idx_bundle_submissions_memo_signature
    ON bundle_submissions (memo_signature);
