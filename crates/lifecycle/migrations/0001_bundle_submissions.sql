-- Tracks each Jito bundle submission through its commitment stages, plus the
-- inter-stage latency deltas used by the failure classifier / decision layer.
CREATE TABLE IF NOT EXISTS bundle_submissions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    bundle_id       TEXT    NOT NULL UNIQUE,
    tip_lamports    INTEGER NOT NULL,

    -- Commitment-stage timestamps (unix millis). NULL until the stage is reached.
    submitted_at    INTEGER NOT NULL,
    processed_at    INTEGER,
    confirmed_at    INTEGER,
    finalized_at    INTEGER,

    -- Slot accounting.
    submitted_slot  INTEGER NOT NULL,
    landed_slot     INTEGER,

    -- Terminal failure reason, NULL while in-flight or on success.
    -- Mirrors `failure::FailureKind` variants as text.
    failure_kind    TEXT,

    -- Inter-stage latency deltas (millis), derived from the timestamps above.
    submit_to_process_ms    INTEGER,
    process_to_confirm_ms   INTEGER,
    confirm_to_finalize_ms  INTEGER
);

CREATE INDEX IF NOT EXISTS idx_bundle_submissions_bundle_id
    ON bundle_submissions (bundle_id);
CREATE INDEX IF NOT EXISTS idx_bundle_submissions_submitted_slot
    ON bundle_submissions (submitted_slot);
