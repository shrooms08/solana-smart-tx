//! Per-bundle lifecycle state machine, driven by stream events and persisted to
//! SQLite.
//!
//! States: `Submitted → Processed → Confirmed → Finalized`, with `Failed`
//! reachable from `Submitted` (rejected at submit, or never landed) or from a
//! landed-but-errored transaction.
//!
//! The hot path (signature → pending record on every `TxStatusUpdate`, and the
//! slot → bundles reverse index on every `SlotUpdate`) is served entirely from
//! in-memory state under a single mutex; SQLite is a **write-through** mirror.
//! On startup the in-memory state is hydrated from the DB (crash recovery).
//!
//! Failure *interpretation* lives in [`failure`]: this crate only routes
//! evidence into `failure::classify` and stores the resulting `Classification`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use failure::{Classification, Confidence, Evidence};
use serde::Serialize;
use sqlx::sqlite::SqlitePool;
use stream::{SlotStatus, StreamEvent};
use submitter::BundleRecord;
use tracing::{debug, info, warn};

/// Embedded migrations (see `migrations/`).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Lifecycle tracker tuning.
#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    /// A still-`Submitted` bundle whose blockhash age
    /// (`last_observed_slot - blockhash_fetched_at_slot`) exceeds this is
    /// declared timed-out by [`LifecycleTracker::check_timeouts`].
    pub timeout_slots: u64,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self { timeout_slots: 160 }
    }
}

// ---------------------------------------------------------------------------
// In-memory state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Submitted,
    Processed,
    Confirmed,
    Finalized,
    Failed,
}

impl Stage {
    fn rank(self) -> u8 {
        match self {
            Stage::Submitted => 0,
            Stage::Processed => 1,
            Stage::Confirmed => 2,
            Stage::Finalized => 3,
            Stage::Failed => 4,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Stage::Submitted => "Submitted",
            Stage::Processed => "Processed",
            Stage::Confirmed => "Confirmed",
            Stage::Finalized => "Finalized",
            Stage::Failed => "Failed",
        }
    }

    fn parse(s: &str) -> Option<Stage> {
        Some(match s {
            "Submitted" => Stage::Submitted,
            "Processed" => Stage::Processed,
            "Confirmed" => Stage::Confirmed,
            "Finalized" => Stage::Finalized,
            "Failed" => Stage::Failed,
            _ => return None,
        })
    }
}

/// A bundle currently tracked in memory (non-terminal).
#[derive(Debug, Clone)]
struct Pending {
    memo_signature: String,
    state: Stage,
    landed_slot: Option<u64>,
    submitted_slot: u64,
    blockhash_fetched_at_slot: u64,
    submitted_at_ms: i64,
    processed_at_ms: Option<i64>,
    confirmed_at_ms: Option<i64>,
    tip_lamports: u64,
    tip_p50: Option<u64>,
    tip_p75: Option<u64>,
    /// Last Jito `getInflightBundleStatuses` verdict (poller-recorded), used by the
    /// never-landed classifier to distinguish AuctionLost from ExpiredBlockhash.
    jito_inflight: Option<String>,
}

#[derive(Debug, Default)]
struct Inner {
    /// memo signature -> row id (fast lookup on every TxStatusUpdate).
    by_sig: HashMap<String, i64>,
    /// row id -> pending record.
    pending: HashMap<i64, Pending>,
    /// landed_slot -> ids awaiting Confirmed (reverse index; one slot can hold
    /// many bundles).
    confirm_index: BTreeMap<u64, HashSet<i64>>,
    /// landed_slot -> ids awaiting Finalized.
    finalize_index: BTreeMap<u64, HashSet<i64>>,
    /// Highest slot seen on the stream — the lifecycle clock.
    last_observed_slot: u64,
}

// ---------------------------------------------------------------------------
// Reconcile RPC seam
// ---------------------------------------------------------------------------

/// Commitment level reported by `getSignatureStatuses`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationLevel {
    Processed,
    Confirmed,
    Finalized,
}

/// A signature's on-chain status, normalized for [`LifecycleTracker::reconcile`].
#[derive(Debug, Clone)]
pub struct SigStatus {
    pub slot: u64,
    pub confirmation: ConfirmationLevel,
    /// `Some` if the transaction landed but errored: hex-encoded bincode
    /// `TransactionError`, in the same form the stream produces (so
    /// `failure::classify` is the single interpreter).
    pub err_hex: Option<String>,
}

/// The reconcile RPC seam. Mocked in tests; implemented for the real nonblocking
/// `RpcClient` below.
pub trait SignatureStatusSource {
    fn signature_statuses(
        &self,
        signatures: &[String],
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<Option<SigStatus>>>> + Send;
}

impl SignatureStatusSource for solana_rpc_client::nonblocking::rpc_client::RpcClient {
    async fn signature_statuses(
        &self,
        signatures: &[String],
    ) -> anyhow::Result<Vec<Option<SigStatus>>> {
        use solana_sdk::signature::Signature;
        use solana_transaction_status_client_types::TransactionConfirmationStatus as Tcs;
        use std::str::FromStr;

        let sigs = signatures
            .iter()
            .map(|s| Signature::from_str(s))
            .collect::<Result<Vec<_>, _>>()?;
        let response = self.get_signature_statuses(&sigs).await?;

        Ok(response
            .value
            .into_iter()
            .map(|opt| {
                opt.map(|ts| {
                    let confirmation = match ts.confirmation_status {
                        Some(Tcs::Finalized) => ConfirmationLevel::Finalized,
                        Some(Tcs::Confirmed) => ConfirmationLevel::Confirmed,
                        _ => ConfirmationLevel::Processed,
                    };
                    let err_hex = ts
                        .err
                        .and_then(|e| bincode::serialize(&e).ok())
                        .map(|bytes| to_hex(&bytes));
                    SigStatus {
                        slot: ts.slot,
                        confirmation,
                        err_hex,
                    }
                })
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Reports / exports
// ---------------------------------------------------------------------------

/// Outcome of a [`LifecycleTracker::reconcile`] pass.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ReconcileReport {
    /// Number of non-terminal bundles checked.
    pub checked: usize,
    /// Transitions applied as a result.
    pub transitions: Vec<ReconcileTransition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileTransition {
    pub id: i64,
    pub to_stage: String,
    pub slot: u64,
}

/// Paths written by [`LifecycleTracker::export_log`].
#[derive(Debug, Clone)]
pub struct ExportPaths {
    pub json: PathBuf,
    pub md: PathBuf,
}

/// One exported row (the full persisted record).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LogRow {
    pub id: i64,
    pub bundle_id: String,
    pub memo_signature: String,
    pub tip_signature: String,
    pub tip_account: String,
    pub blockhash: String,
    pub blockhash_fetched_at_slot: i64,
    pub submitted_slot: i64,
    pub tip_lamports: i64,
    pub fault_injected: Option<String>,
    pub tip_p50_at_submit: Option<i64>,
    pub tip_p75_at_submit: Option<i64>,
    pub status: String,
    pub submitted_at: i64,
    pub processed_at: Option<i64>,
    pub confirmed_at: Option<i64>,
    pub finalized_at: Option<i64>,
    pub landed_slot: Option<i64>,
    pub submit_to_process_ms: Option<i64>,
    pub process_to_confirm_ms: Option<i64>,
    pub confirm_to_finalize_ms: Option<i64>,
    pub failure_kind: Option<String>,
    pub failure_confidence: Option<String>,
    pub failure_rationale: Option<String>,
    pub processed_source: Option<String>,
    pub confirmed_source: Option<String>,
    pub finalized_source: Option<String>,
    /// Last Jito `getInflightBundleStatuses` verdict (poller-recorded).
    pub jito_inflight_status: Option<String>,
}

// ---------------------------------------------------------------------------
// Write-through DB operations
// ---------------------------------------------------------------------------

/// A single persisted transition produced by the in-memory state machine.
#[derive(Debug, Clone)]
enum Write {
    Processed {
        id: i64,
        processed_at: i64,
        landed_slot: i64,
        submit_to_process_ms: Option<i64>,
        source: &'static str,
    },
    Confirmed {
        id: i64,
        confirmed_at: i64,
        process_to_confirm_ms: Option<i64>,
        source: &'static str,
    },
    Finalized {
        id: i64,
        finalized_at: i64,
        confirm_to_finalize_ms: Option<i64>,
        source: &'static str,
    },
    Failed {
        id: i64,
        kind: String,
        confidence: String,
        rationale: String,
        landed_slot: Option<i64>,
        processed_at: Option<i64>,
        submit_to_process_ms: Option<i64>,
        failed_at: i64,
    },
}

async fn persist(pool: &SqlitePool, write: &Write) -> anyhow::Result<()> {
    match write {
        Write::Processed {
            id,
            processed_at,
            landed_slot,
            submit_to_process_ms,
            source,
        } => {
            sqlx::query(
                "UPDATE bundle_submissions SET status='Processed', processed_at=?, \
                 landed_slot=?, submit_to_process_ms=?, processed_source=? WHERE id=?",
            )
            .bind(processed_at)
            .bind(landed_slot)
            .bind(submit_to_process_ms)
            .bind(*source)
            .bind(id)
            .execute(pool)
            .await?;
        }
        Write::Confirmed {
            id,
            confirmed_at,
            process_to_confirm_ms,
            source,
        } => {
            sqlx::query(
                "UPDATE bundle_submissions SET status='Confirmed', confirmed_at=?, \
                 process_to_confirm_ms=?, confirmed_source=? WHERE id=?",
            )
            .bind(confirmed_at)
            .bind(process_to_confirm_ms)
            .bind(*source)
            .bind(id)
            .execute(pool)
            .await?;
        }
        Write::Finalized {
            id,
            finalized_at,
            confirm_to_finalize_ms,
            source,
        } => {
            sqlx::query(
                "UPDATE bundle_submissions SET status='Finalized', finalized_at=?, \
                 confirm_to_finalize_ms=?, finalized_source=? WHERE id=?",
            )
            .bind(finalized_at)
            .bind(confirm_to_finalize_ms)
            .bind(*source)
            .bind(id)
            .execute(pool)
            .await?;
        }
        Write::Failed {
            id,
            kind,
            confidence,
            rationale,
            landed_slot,
            processed_at,
            submit_to_process_ms,
            failed_at,
        } => {
            sqlx::query(
                "UPDATE bundle_submissions SET status='Failed', failure_kind=?, \
                 failure_confidence=?, failure_rationale=?, \
                 landed_slot=COALESCE(?, landed_slot), \
                 processed_at=COALESCE(?, processed_at), \
                 submit_to_process_ms=COALESCE(?, submit_to_process_ms), \
                 finalized_at=COALESCE(finalized_at, ?) WHERE id=?",
            )
            .bind(kind)
            .bind(confidence)
            .bind(rationale)
            .bind(landed_slot)
            .bind(processed_at)
            .bind(submit_to_process_ms)
            .bind(failed_at)
            .bind(id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tracker
// ---------------------------------------------------------------------------

/// The lifecycle tracker. Cheap to clone (shared pool + state).
#[derive(Clone)]
pub struct LifecycleTracker {
    pool: SqlitePool,
    inner: Arc<Mutex<Inner>>,
    config: LifecycleConfig,
}

impl LifecycleTracker {
    /// Build a tracker over `pool`: runs migrations, then hydrates the in-memory
    /// state from any non-terminal rows already in the DB (crash recovery).
    pub async fn new(pool: SqlitePool, config: LifecycleConfig) -> anyhow::Result<Self> {
        MIGRATOR.run(&pool).await?;
        let inner = hydrate(&pool).await?;
        Ok(Self {
            pool,
            inner: Arc::new(Mutex::new(inner)),
            config,
        })
    }

    /// Record a freshly submitted bundle, stamping the tip context at submit
    /// time (for later `NeverLanded` evidence). Returns the row id.
    pub async fn record_submission(
        &self,
        record: &BundleRecord,
        tip_p50: Option<u64>,
        tip_p75: Option<u64>,
    ) -> anyhow::Result<i64> {
        let submitted_at = to_millis(record.submitted_at);
        let slot = record.blockhash_fetched_at_slot as i64;

        let id: i64 = sqlx::query_scalar(
            "INSERT INTO bundle_submissions (\
                bundle_id, memo_signature, tip_signature, tip_account, blockhash, \
                blockhash_fetched_at_slot, submitted_slot, tip_lamports, fault_injected, \
                tip_p50_at_submit, tip_p75_at_submit, status, submitted_at) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?, 'Submitted', ?) RETURNING id",
        )
        .bind(&record.bundle_id)
        .bind(&record.memo_signature)
        .bind(&record.tip_signature)
        .bind(&record.tip_account)
        .bind(&record.blockhash)
        .bind(slot)
        .bind(slot)
        .bind(record.tip_lamports as i64)
        .bind(&record.fault_injected)
        .bind(tip_p50.map(|v| v as i64))
        .bind(tip_p75.map(|v| v as i64))
        .bind(submitted_at)
        .fetch_one(&self.pool)
        .await?;

        let mut inner = self.inner.lock().unwrap();
        inner.by_sig.insert(record.memo_signature.clone(), id);
        inner.pending.insert(
            id,
            Pending {
                memo_signature: record.memo_signature.clone(),
                state: Stage::Submitted,
                landed_slot: None,
                submitted_slot: record.blockhash_fetched_at_slot,
                blockhash_fetched_at_slot: record.blockhash_fetched_at_slot,
                submitted_at_ms: submitted_at,
                processed_at_ms: None,
                confirmed_at_ms: None,
                tip_lamports: record.tip_lamports,
                tip_p50,
                tip_p75,
                jito_inflight: None,
            },
        );
        debug!(id, bundle_id = %record.bundle_id, "recorded submission");
        Ok(id)
    }

    /// Record the latest Jito `getInflightBundleStatuses` verdict for a bundle
    /// (called by the bundle-status poller). Persists it and updates the in-memory
    /// pending record so a later never-landed timeout classifies AuctionLost vs
    /// ExpiredBlockhash correctly. No-op for already-terminal/unknown rows.
    pub async fn record_jito_status(&self, id: i64, status: &str) -> anyhow::Result<()> {
        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(p) = inner.pending.get_mut(&id) {
                p.jito_inflight = Some(status.to_string());
            }
        }
        sqlx::query("UPDATE bundle_submissions SET jito_inflight_status=? WHERE id=?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// `sendBundle` failed synchronously: classify the raw error as a
    /// `SubmitRejection` and mark the row `Failed`.
    pub async fn record_rejection(&self, record_id: i64, raw_error: &str) -> anyhow::Result<()> {
        let classification = failure::classify(&Evidence::SubmitRejection {
            raw_error: raw_error.to_string(),
        });
        {
            let mut inner = self.inner.lock().unwrap();
            forget(&mut inner, record_id);
        }
        let write = failed_write(record_id, &classification, None, None, None, now_millis());
        persist(&self.pool, &write).await?;
        info!(id = record_id, kind = %classification.kind_label(), "recorded rejection");
        Ok(())
    }

    /// Ingest one stream event (cheap, in-memory) and **spawn** the resulting
    /// write-through transitions as background tasks. Must be called from within
    /// a Tokio runtime (it sits behind the stream channel in the orchestrator).
    pub fn ingest(&self, event: &StreamEvent) {
        let writes = {
            let mut inner = self.inner.lock().unwrap();
            apply_event(&mut inner, event)
        };
        for write in writes {
            let pool = self.pool.clone();
            tokio::spawn(async move {
                if let Err(err) = persist(&pool, &write).await {
                    warn!(error = %err, "lifecycle write-through failed");
                }
            });
        }
    }

    /// Like [`ingest`](Self::ingest) but awaits persistence — used by tests and
    /// when the caller wants back-pressure instead of fire-and-forget.
    pub async fn ingest_and_persist(&self, event: &StreamEvent) -> anyhow::Result<()> {
        let writes = {
            let mut inner = self.inner.lock().unwrap();
            apply_event(&mut inner, event)
        };
        for write in writes {
            persist(&self.pool, &write).await?;
        }
        Ok(())
    }

    /// Time out bundles still `Submitted` whose blockhash has aged past
    /// `config.timeout_slots`: assemble `NeverLanded` evidence (with the stamped
    /// tip context), classify, and mark `Failed`. Returns the affected row ids.
    pub async fn check_timeouts(&self) -> anyhow::Result<Vec<i64>> {
        let mut writes = Vec::new();
        let mut ids = Vec::new();
        {
            let mut inner = self.inner.lock().unwrap();
            let last = inner.last_observed_slot;
            let threshold = self.config.timeout_slots;

            let timed_out: Vec<i64> = inner
                .pending
                .iter()
                .filter(|(_, p)| {
                    p.state == Stage::Submitted
                        && last.saturating_sub(p.blockhash_fetched_at_slot) > threshold
                })
                .map(|(id, _)| *id)
                .collect();

            for id in timed_out {
                let p = inner.pending.get(&id).expect("present").clone();
                let evidence = Evidence::NeverLanded {
                    submitted_slot: p.submitted_slot,
                    blockhash_fetched_at_slot: p.blockhash_fetched_at_slot,
                    last_observed_slot: last,
                    tip_lamports: p.tip_lamports,
                    tip_p50_at_submit: p.tip_p50,
                    tip_p75_at_submit: p.tip_p75,
                    jito_inflight: failure::JitoInflight::from_status_str(p.jito_inflight.as_deref()),
                };
                let classification = failure::classify(&evidence);
                forget(&mut inner, id);
                writes.push(failed_write(id, &classification, None, None, None, now_millis()));
                ids.push(id);
            }
        }
        for write in &writes {
            persist(&self.pool, write).await?;
        }
        if !ids.is_empty() {
            info!(count = ids.len(), "timed out never-landed bundles");
        }
        Ok(ids)
    }

    /// Reconcile non-terminal bundles against `getSignatureStatuses` after a
    /// stream gap: apply any missed Processed/Confirmed/Finalized transitions
    /// (flagged `source='reconcile'`) and report what changed.
    pub async fn reconcile<S: SignatureStatusSource>(
        &self,
        source: &S,
    ) -> anyhow::Result<ReconcileReport> {
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT id, memo_signature, status FROM bundle_submissions \
             WHERE status IN ('Submitted','Processed','Confirmed')",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut report = ReconcileReport {
            checked: rows.len(),
            transitions: Vec::new(),
        };
        if rows.is_empty() {
            return Ok(report);
        }

        let sigs: Vec<String> = rows.iter().map(|(_, sig, _)| sig.clone()).collect();
        let statuses = source.signature_statuses(&sigs).await?;

        for ((id, _sig, status), maybe) in rows.iter().zip(statuses) {
            let Some(st) = maybe else { continue };
            let id = *id;
            let current = Stage::parse(status).unwrap_or(Stage::Submitted);

            // Landed-but-errored: route through failure::classify and fail it.
            if let Some(hex) = &st.err_hex {
                let classification = failure::classify(&Evidence::OnChainError {
                    raw_error_hex: hex.clone(),
                    slot: st.slot,
                });
                let write = {
                    let mut inner = self.inner.lock().unwrap();
                    let sub = inner.pending.get(&id).map(|p| p.submitted_at_ms);
                    forget(&mut inner, id);
                    let now = now_millis();
                    failed_write(
                        id,
                        &classification,
                        Some(st.slot as i64),
                        Some(now),
                        sub.map(|s| now - s),
                        now,
                    )
                };
                persist(&self.pool, &write).await?;
                report.transitions.push(ReconcileTransition {
                    id,
                    to_stage: "Failed".to_string(),
                    slot: st.slot,
                });
                continue;
            }

            let target = match st.confirmation {
                ConfirmationLevel::Processed => Stage::Processed,
                ConfirmationLevel::Confirmed => Stage::Confirmed,
                ConfirmationLevel::Finalized => Stage::Finalized,
            };

            let writes = {
                let mut inner = self.inner.lock().unwrap();
                let now = now_millis();
                let mut ws = Vec::new();
                if target.rank() >= Stage::Processed.rank()
                    && current.rank() < Stage::Processed.rank()
                {
                    if let Some(w) = mark_processed(&mut inner, id, st.slot, now, "reconcile") {
                        ws.push((w, Stage::Processed));
                    }
                }
                if target.rank() >= Stage::Confirmed.rank()
                    && current.rank() < Stage::Confirmed.rank()
                {
                    if let Some(w) = mark_confirmed(&mut inner, id, now, "reconcile") {
                        ws.push((w, Stage::Confirmed));
                    }
                }
                if target.rank() >= Stage::Finalized.rank()
                    && current.rank() < Stage::Finalized.rank()
                {
                    if let Some(w) = mark_finalized(&mut inner, id, now, "reconcile") {
                        ws.push((w, Stage::Finalized));
                    }
                }
                ws
            };

            for (write, stage) in writes {
                persist(&self.pool, &write).await?;
                report.transitions.push(ReconcileTransition {
                    id,
                    to_stage: stage.as_str().to_string(),
                    slot: st.slot,
                });
            }
        }

        info!(
            checked = report.checked,
            applied = report.transitions.len(),
            "reconcile complete"
        );
        Ok(report)
    }

    /// Export the full log as `lifecycle_log.json` (all records) and
    /// `lifecycle_log.md` (a human-readable table). One command, the judged
    /// artifact.
    pub async fn export_log(&self, dir: &Path) -> anyhow::Result<ExportPaths> {
        let rows: Vec<LogRow> = sqlx::query_as("SELECT * FROM bundle_submissions ORDER BY id")
            .fetch_all(&self.pool)
            .await?;

        tokio::fs::create_dir_all(dir).await?;
        let json_path = dir.join("lifecycle_log.json");
        let md_path = dir.join("lifecycle_log.md");

        tokio::fs::write(&json_path, serde_json::to_vec_pretty(&rows)?).await?;
        tokio::fs::write(&md_path, render_markdown(&rows)).await?;

        info!(records = rows.len(), dir = %dir.display(), "exported lifecycle log");
        Ok(ExportPaths {
            json: json_path,
            md: md_path,
        })
    }
}

// ---------------------------------------------------------------------------
// Event application (in-memory) — the hot path
// ---------------------------------------------------------------------------

fn apply_event(inner: &mut Inner, event: &StreamEvent) -> Vec<Write> {
    match event {
        StreamEvent::Slot(slot) => {
            inner.last_observed_slot = inner.last_observed_slot.max(slot.slot);
            let at = to_millis(slot.received_at);
            match slot.status {
                // Confirmation is monotonic: a Confirmed slot N confirms every
                // landed bundle in slots <= N that's still Processed.
                SlotStatus::Confirmed => {
                    let ids = drain_le(&mut inner.confirm_index, slot.slot);
                    ids.into_iter()
                        .filter_map(|id| mark_confirmed(inner, id, at, "stream"))
                        .collect()
                }
                // Finalized slot N finalizes everything landed in slots <= N —
                // including bundles whose Confirmed update we never saw.
                SlotStatus::Finalized => {
                    let mut ids = drain_le(&mut inner.finalize_index, slot.slot);
                    ids.extend(drain_le(&mut inner.confirm_index, slot.slot));
                    ids.into_iter()
                        .filter_map(|id| mark_finalized(inner, id, at, "stream"))
                        .collect()
                }
                _ => Vec::new(),
            }
        }
        StreamEvent::TxStatus(tx) => {
            let Some(&id) = inner.by_sig.get(&tx.signature) else {
                return Vec::new(); // not one of ours
            };
            let at = to_millis(tx.received_at);
            match &tx.err {
                None => mark_processed(inner, id, tx.slot, at, "stream")
                    .into_iter()
                    .collect(),
                Some(hex) => {
                    let classification = failure::classify(&Evidence::OnChainError {
                        raw_error_hex: hex.clone(),
                        slot: tx.slot,
                    });
                    fail_landed(inner, id, &classification, tx.slot, at)
                        .into_iter()
                        .collect()
                }
            }
        }
    }
}

fn mark_processed(
    inner: &mut Inner,
    id: i64,
    slot: u64,
    at: i64,
    source: &'static str,
) -> Option<Write> {
    let submit_to_process_ms;
    {
        let p = inner.pending.get_mut(&id)?;
        if p.state != Stage::Submitted {
            return None; // idempotent: ignore duplicate tx updates
        }
        p.state = Stage::Processed;
        p.processed_at_ms = Some(at);
        p.landed_slot = Some(slot);
        submit_to_process_ms = Some(at - p.submitted_at_ms);
    }
    inner.confirm_index.entry(slot).or_default().insert(id);
    Some(Write::Processed {
        id,
        processed_at: at,
        landed_slot: slot as i64,
        submit_to_process_ms,
        source,
    })
}

fn mark_confirmed(inner: &mut Inner, id: i64, at: i64, source: &'static str) -> Option<Write> {
    let (process_to_confirm_ms, landed);
    {
        let p = inner.pending.get_mut(&id)?;
        if p.state != Stage::Processed {
            return None;
        }
        p.state = Stage::Confirmed;
        p.confirmed_at_ms = Some(at);
        process_to_confirm_ms = p.processed_at_ms.map(|x| at - x);
        landed = p.landed_slot;
    }
    if let Some(ls) = landed {
        inner.finalize_index.entry(ls).or_default().insert(id);
    }
    Some(Write::Confirmed {
        id,
        confirmed_at: at,
        process_to_confirm_ms,
        source,
    })
}

fn mark_finalized(inner: &mut Inner, id: i64, at: i64, source: &'static str) -> Option<Write> {
    let confirm_to_finalize_ms;
    {
        let p = inner.pending.get(&id)?;
        if p.state != Stage::Processed && p.state != Stage::Confirmed {
            return None;
        }
        confirm_to_finalize_ms = p.confirmed_at_ms.map(|c| at - c);
    }
    forget(inner, id);
    Some(Write::Finalized {
        id,
        finalized_at: at,
        confirm_to_finalize_ms,
        source,
    })
}

fn fail_landed(
    inner: &mut Inner,
    id: i64,
    classification: &Classification,
    slot: u64,
    at: i64,
) -> Option<Write> {
    let submitted_at;
    {
        let p = inner.pending.get(&id)?;
        if p.state != Stage::Submitted && p.state != Stage::Processed {
            return None;
        }
        submitted_at = p.submitted_at_ms;
    }
    forget(inner, id);
    Some(failed_write(
        id,
        classification,
        Some(slot as i64),
        Some(at),
        Some(at - submitted_at),
        at,
    ))
}

/// Remove a bundle from all in-memory indices (it became terminal).
fn forget(inner: &mut Inner, id: i64) {
    if let Some(p) = inner.pending.remove(&id) {
        inner.by_sig.remove(&p.memo_signature);
        if let Some(ls) = p.landed_slot {
            remove_id(&mut inner.confirm_index, ls, id);
            remove_id(&mut inner.finalize_index, ls, id);
        }
    }
}

fn remove_id(index: &mut BTreeMap<u64, HashSet<i64>>, slot: u64, id: i64) {
    if let Some(set) = index.get_mut(&slot) {
        set.remove(&id);
        if set.is_empty() {
            index.remove(&slot);
        }
    }
}

/// Drain (remove and return) all ids keyed at slots `<= n`.
fn drain_le(index: &mut BTreeMap<u64, HashSet<i64>>, n: u64) -> Vec<i64> {
    let keys: Vec<u64> = index.range(..=n).map(|(k, _)| *k).collect();
    let mut ids = Vec::new();
    for k in keys {
        if let Some(set) = index.remove(&k) {
            ids.extend(set);
        }
    }
    ids
}

fn failed_write(
    id: i64,
    classification: &Classification,
    landed_slot: Option<i64>,
    processed_at: Option<i64>,
    submit_to_process_ms: Option<i64>,
    failed_at: i64,
) -> Write {
    Write::Failed {
        id,
        kind: classification.kind_label(),
        confidence: confidence_label(&classification.confidence),
        rationale: classification.rationale.clone(),
        landed_slot,
        processed_at,
        submit_to_process_ms,
        failed_at,
    }
}

// ---------------------------------------------------------------------------
// Hydration (crash recovery)
// ---------------------------------------------------------------------------

async fn hydrate(pool: &SqlitePool) -> anyhow::Result<Inner> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: i64,
        memo_signature: String,
        status: String,
        submitted_slot: i64,
        blockhash_fetched_at_slot: i64,
        landed_slot: Option<i64>,
        submitted_at: i64,
        processed_at: Option<i64>,
        confirmed_at: Option<i64>,
        tip_lamports: i64,
        tip_p50_at_submit: Option<i64>,
        tip_p75_at_submit: Option<i64>,
        jito_inflight_status: Option<String>,
    }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, memo_signature, status, submitted_slot, blockhash_fetched_at_slot, \
         landed_slot, submitted_at, processed_at, confirmed_at, tip_lamports, \
         tip_p50_at_submit, tip_p75_at_submit, jito_inflight_status FROM bundle_submissions \
         WHERE status IN ('Submitted','Processed','Confirmed')",
    )
    .fetch_all(pool)
    .await?;

    let mut inner = Inner::default();
    for row in rows {
        let state = Stage::parse(&row.status).unwrap_or(Stage::Submitted);
        let landed_slot = row.landed_slot.map(|s| s as u64);
        inner.by_sig.insert(row.memo_signature.clone(), row.id);
        inner.pending.insert(
            row.id,
            Pending {
                memo_signature: row.memo_signature,
                state,
                landed_slot,
                submitted_slot: row.submitted_slot as u64,
                blockhash_fetched_at_slot: row.blockhash_fetched_at_slot as u64,
                submitted_at_ms: row.submitted_at,
                processed_at_ms: row.processed_at,
                confirmed_at_ms: row.confirmed_at,
                tip_lamports: row.tip_lamports as u64,
                tip_p50: row.tip_p50_at_submit.map(|v| v as u64),
                tip_p75: row.tip_p75_at_submit.map(|v| v as u64),
                jito_inflight: row.jito_inflight_status,
            },
        );
        match (state, landed_slot) {
            (Stage::Processed, Some(ls)) => {
                inner.confirm_index.entry(ls).or_default().insert(row.id);
            }
            (Stage::Confirmed, Some(ls)) => {
                inner.finalize_index.entry(ls).or_default().insert(row.id);
            }
            _ => {}
        }
        inner.last_observed_slot = inner
            .last_observed_slot
            .max(landed_slot.unwrap_or(0))
            .max(row.submitted_slot as u64);
    }
    if !inner.pending.is_empty() {
        info!(hydrated = inner.pending.len(), "hydrated lifecycle state from DB");
    }
    Ok(inner)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_millis() -> i64 {
    to_millis(SystemTime::now())
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn confidence_label(confidence: &Confidence) -> String {
    match confidence {
        Confidence::Certain => "Certain".to_string(),
        Confidence::Likely => "Likely".to_string(),
        Confidence::Ambiguous { alternatives } => {
            if alternatives.is_empty() {
                "Ambiguous".to_string()
            } else {
                let alts = alternatives
                    .iter()
                    .map(|k| format!("{k:?}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Ambiguous(alt: {alts})")
            }
        }
    }
}

/// Small extension so call sites read cleanly.
trait ClassificationExt {
    fn kind_label(&self) -> String;
}
impl ClassificationExt for Classification {
    fn kind_label(&self) -> String {
        format!("{:?}", self.kind)
    }
}

fn render_markdown(rows: &[LogRow]) -> String {
    let mut out = String::new();
    out.push_str("# Bundle lifecycle log\n\n");
    out.push_str(
        "| id | bundle_id | status | sub_slot | landed_slot | tip (lamports) | \
         submitted_at | processed_at | confirmed_at | finalized_at | \
         s→p ms | p→c ms | c→f ms | failure |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for r in rows {
        let failure = match (&r.failure_kind, &r.failure_confidence, &r.failure_rationale) {
            (Some(k), Some(c), Some(rat)) => format!("{k} ({c}): {rat}"),
            (Some(k), _, _) => k.clone(),
            _ => String::new(),
        };
        let opt = |v: &Option<i64>| v.map(|x| x.to_string()).unwrap_or_default();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.id,
            r.bundle_id,
            r.status,
            r.submitted_slot,
            opt(&r.landed_slot),
            r.tip_lamports,
            r.submitted_at,
            opt(&r.processed_at),
            opt(&r.confirmed_at),
            opt(&r.finalized_at),
            opt(&r.submit_to_process_ms),
            opt(&r.process_to_confirm_ms),
            opt(&r.confirm_to_finalize_ms),
            failure.replace('|', "\\|"),
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Tests (offline, in-memory SQLite)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::Duration;
    use stream::{SlotUpdate, TxStatusUpdate};

    async fn mem_pool() -> SqlitePool {
        // Single connection so the in-memory DB persists across tracker clones.
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory pool")
    }

    fn at_ms(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    fn record(bundle: &str, sig: &str, slot: u64, tip: u64, submitted_ms: u64) -> BundleRecord {
        BundleRecord {
            bundle_id: bundle.to_string(),
            tip_lamports: tip,
            tip_account: "TipAcct11111111111111111111111111111111111".to_string(),
            memo_signature: sig.to_string(),
            tip_signature: format!("{sig}-tip"),
            blockhash: "Bhash1111111111111111111111111111111111111".to_string(),
            blockhash_fetched_at_slot: slot,
            submitted_at: at_ms(submitted_ms),
            fault_injected: None,
        }
    }

    fn slot_event(slot: u64, status: SlotStatus, at: u64) -> StreamEvent {
        StreamEvent::Slot(SlotUpdate {
            slot,
            status,
            received_at: at_ms(at),
        })
    }

    fn tx_event(sig: &str, slot: u64, err: Option<String>, at: u64) -> StreamEvent {
        StreamEvent::TxStatus(TxStatusUpdate {
            signature: sig.to_string(),
            slot,
            err,
            received_at: at_ms(at),
        })
    }

    async fn fetch(pool: &SqlitePool, id: i64) -> LogRow {
        sqlx::query_as("SELECT * FROM bundle_submissions WHERE id=?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn happy_path_full_transition_with_deltas() {
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();

        let id = t
            .record_submission(
                &record("b1", "sigA", 1000, 50_000, 1000),
                Some(40_000),
                Some(60_000),
            )
            .await
            .unwrap();

        // Processed at landed slot 1005, t=1100 -> s2p = 100.
        t.ingest_and_persist(&tx_event("sigA", 1005, None, 1100))
            .await
            .unwrap();
        // Confirmed: slot 1005 confirmed at t=1300 -> p2c = 200.
        t.ingest_and_persist(&slot_event(1005, SlotStatus::Confirmed, 1300))
            .await
            .unwrap();
        // Finalized: slot 1010 (>=1005) finalized at t=1600 -> c2f = 300.
        t.ingest_and_persist(&slot_event(1010, SlotStatus::Finalized, 1600))
            .await
            .unwrap();

        let row = fetch(&pool, id).await;
        assert_eq!(row.status, "Finalized");
        assert_eq!(row.landed_slot, Some(1005));
        assert_eq!(row.submit_to_process_ms, Some(100));
        assert_eq!(row.process_to_confirm_ms, Some(200));
        assert_eq!(row.confirm_to_finalize_ms, Some(300));
        assert_eq!(row.processed_source.as_deref(), Some("stream"));
        assert_eq!(row.finalized_source.as_deref(), Some("stream"));
    }

    #[tokio::test]
    async fn landed_but_failed_path() {
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        let id = t
            .record_submission(&record("b2", "sigB", 2000, 10_000, 0), None, None)
            .await
            .unwrap();

        // "00000000" = bincode u32 enum index 0 -> TransactionError::AccountInUse.
        t.ingest_and_persist(&tx_event("sigB", 2003, Some("00000000".to_string()), 50))
            .await
            .unwrap();

        let row = fetch(&pool, id).await;
        assert_eq!(row.status, "Failed");
        assert_eq!(row.failure_kind.as_deref(), Some("BundleFailure"));
        assert!(row.failure_confidence.is_some());
        assert_eq!(row.landed_slot, Some(2003));
    }

    #[tokio::test]
    async fn timeout_produces_never_landed_with_tip_context() {
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        // Competitive tip, blockhash valid at submission, no Jito status recorded.
        // The blockhash only ages while waiting -> AuctionLost (inferred), NOT
        // ExpiredBlockhash (the expiry is a downstream symptom).
        let id = t
            .record_submission(
                &record("b3", "sigC", 1000, 50_000, 0),
                Some(50_000),
                Some(70_000),
            )
            .await
            .unwrap();

        // Advance the clock to slot 1161 (age 161 > 160).
        t.ingest_and_persist(&slot_event(1161, SlotStatus::Processed, 10))
            .await
            .unwrap();

        let timed_out = t.check_timeouts().await.unwrap();
        assert_eq!(timed_out, vec![id]);

        let row = fetch(&pool, id).await;
        assert_eq!(row.status, "Failed");
        assert_eq!(row.failure_kind.as_deref(), Some("AuctionLost"));
        assert!(row.failure_rationale.unwrap().contains("tip 50000"));
    }

    #[tokio::test]
    async fn timeout_with_recorded_jito_invalid_status_is_auction_lost() {
        // End-to-end plumbing: the bundle-status poller recorded Jito 'Invalid';
        // when the bundle later times out as never-landed, the classifier sees that
        // status and classifies AuctionLost (Certain) — not ExpiredBlockhash.
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        let id = t
            .record_submission(
                &record("b3i", "sigCi", 1000, 50_000, 0),
                Some(50_000),
                Some(70_000),
            )
            .await
            .unwrap();

        // Poller observes Invalid (accepted but not in Jito's system).
        t.record_jito_status(id, "Invalid").await.unwrap();
        assert_eq!(
            fetch(&pool, id).await.jito_inflight_status.as_deref(),
            Some("Invalid")
        );

        t.ingest_and_persist(&slot_event(1161, SlotStatus::Processed, 10))
            .await
            .unwrap();
        assert_eq!(t.check_timeouts().await.unwrap(), vec![id]);

        let row = fetch(&pool, id).await;
        assert_eq!(row.failure_kind.as_deref(), Some("AuctionLost"));
        assert_eq!(row.failure_confidence.as_deref(), Some("Certain"));
        let rationale = row.failure_rationale.unwrap();
        assert!(rationale.contains("Invalid"));
        assert!(rationale.to_lowercase().contains("auction") || rationale.contains("did not win"));
    }

    #[tokio::test]
    async fn timeout_does_not_fire_before_threshold() {
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        let _id = t
            .record_submission(&record("b3b", "sigCb", 1000, 1, 0), Some(1000), None)
            .await
            .unwrap();
        // age 160 == threshold, not strictly greater -> no timeout.
        t.ingest_and_persist(&slot_event(1160, SlotStatus::Processed, 10))
            .await
            .unwrap();
        assert!(t.check_timeouts().await.unwrap().is_empty());
    }

    // Mock RPC seam for reconcile.
    struct MockRpc {
        statuses: HashMap<String, SigStatus>,
    }

    impl SignatureStatusSource for MockRpc {
        async fn signature_statuses(
            &self,
            signatures: &[String],
        ) -> anyhow::Result<Vec<Option<SigStatus>>> {
            Ok(signatures
                .iter()
                .map(|s| self.statuses.get(s).cloned())
                .collect())
        }
    }

    #[tokio::test]
    async fn reconcile_applies_missed_transitions() {
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        let id = t
            .record_submission(&record("b4", "sigD", 3000, 20_000, 0), None, None)
            .await
            .unwrap();
        // Bring it to Processed via the stream...
        t.ingest_and_persist(&tx_event("sigD", 3005, None, 100))
            .await
            .unwrap();

        // ...then a gap: the chain finalized it, reconcile catches up.
        let mut statuses = HashMap::new();
        statuses.insert(
            "sigD".to_string(),
            SigStatus {
                slot: 3005,
                confirmation: ConfirmationLevel::Finalized,
                err_hex: None,
            },
        );
        let report = t.reconcile(&MockRpc { statuses }).await.unwrap();

        assert_eq!(report.checked, 1);
        assert!(report.transitions.iter().any(|tr| tr.to_stage == "Finalized"));

        let row = fetch(&pool, id).await;
        assert_eq!(row.status, "Finalized");
        assert_eq!(row.confirmed_source.as_deref(), Some("reconcile"));
        assert_eq!(row.finalized_source.as_deref(), Some("reconcile"));
    }

    #[tokio::test]
    async fn export_produces_json_and_table() {
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        let id = t
            .record_submission(
                &record("b5", "sigE", 4000, 12345, 1000),
                Some(10_000),
                None,
            )
            .await
            .unwrap();
        t.ingest_and_persist(&tx_event("sigE", 4001, None, 1100))
            .await
            .unwrap();

        let dir = std::env::temp_dir().join(format!("lifecycle_export_{}", std::process::id()));
        let paths = t.export_log(&dir).await.unwrap();

        let json = std::fs::read_to_string(&paths.json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert_eq!(parsed[0]["bundle_id"], "b5");
        assert_eq!(parsed[0]["id"], id);

        let md = std::fs::read_to_string(&paths.md).unwrap();
        assert!(md.contains("Bundle lifecycle log"));
        assert!(md.contains("b5"));
        assert!(md.contains("12345"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn hydration_after_crash_mid_processed() {
        let pool = mem_pool().await;
        let id = {
            let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
                .await
                .unwrap();
            let id = t
                .record_submission(&record("b6", "sigF", 5000, 30_000, 0), None, None)
                .await
                .unwrap();
            // Advance to Processed, then "crash" (drop the tracker).
            t.ingest_and_persist(&tx_event("sigF", 5005, None, 100))
                .await
                .unwrap();
            id
        };

        // New tracker on the same pool hydrates the Processed bundle into the
        // confirm index, so subsequent slot updates still advance it.
        let t2 = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        t2.ingest_and_persist(&slot_event(5005, SlotStatus::Confirmed, 300))
            .await
            .unwrap();
        t2.ingest_and_persist(&slot_event(5005, SlotStatus::Finalized, 600))
            .await
            .unwrap();

        let row = fetch(&pool, id).await;
        assert_eq!(row.status, "Finalized");
        assert_eq!(row.process_to_confirm_ms, Some(200)); // 300 - 100
        assert_eq!(row.confirm_to_finalize_ms, Some(300)); // 600 - 300
    }

    #[tokio::test]
    async fn one_slot_finalizes_multiple_bundles() {
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        let id1 = t
            .record_submission(&record("m1", "s1", 6000, 1, 0), None, None)
            .await
            .unwrap();
        let id2 = t
            .record_submission(&record("m2", "s2", 6000, 1, 0), None, None)
            .await
            .unwrap();
        // Both land in the same slot 6001.
        t.ingest_and_persist(&tx_event("s1", 6001, None, 10))
            .await
            .unwrap();
        t.ingest_and_persist(&tx_event("s2", 6001, None, 10))
            .await
            .unwrap();
        // A single Finalized for slot 6001 finalizes both (jumping past Confirmed).
        t.ingest_and_persist(&slot_event(6001, SlotStatus::Finalized, 100))
            .await
            .unwrap();

        assert_eq!(fetch(&pool, id1).await.status, "Finalized");
        assert_eq!(fetch(&pool, id2).await.status, "Finalized");
    }

    #[tokio::test]
    async fn rejection_marks_failed() {
        let pool = mem_pool().await;
        let t = LifecycleTracker::new(pool.clone(), LifecycleConfig::default())
            .await
            .unwrap();
        let id = t
            .record_submission(&record("b7", "sigG", 7000, 500, 0), None, None)
            .await
            .unwrap();
        t.record_rejection(id, "Bundle must tip at least 1000 lamports")
            .await
            .unwrap();

        let row = fetch(&pool, id).await;
        assert_eq!(row.status, "Failed");
        assert_eq!(row.failure_kind.as_deref(), Some("FeeTooLow"));
    }
}
