//! Commitment-stage lifecycle tracking + SQLite persistence.
//!
//! Records each bundle submission as it advances processed → confirmed →
//! finalized, persisting timestamps, slots, and inter-stage latency deltas to
//! SQLite via a shared [`sqlx`] connection pool.

use failure::FailureKind;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

/// Embedded migrations (see `migrations/`).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// A bundle's progress through the commitment stages.
///
/// TODO: extend with the fields needed to compute and persist the latency
/// deltas; this mirrors the `bundle_submissions` table.
#[derive(Debug, Clone)]
pub struct BundleLifecycle {
    pub bundle_id: String,
    pub tip_lamports: u64,
    pub submitted_at_ms: i64,
    pub processed_at_ms: Option<i64>,
    pub confirmed_at_ms: Option<i64>,
    pub finalized_at_ms: Option<i64>,
    pub submitted_slot: u64,
    pub landed_slot: Option<u64>,
    pub failure_kind: Option<FailureKind>,
}

/// Persistence handle wrapping a SQLite connection pool.
#[derive(Debug, Clone)]
pub struct LifecycleStore {
    pool: SqlitePool,
}

impl LifecycleStore {
    /// Open (or create) the SQLite database and run migrations.
    ///
    /// `url` is a sqlx SQLite URL, e.g. `sqlite://lifecycle.db?mode=rwc`.
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    /// Access the underlying pool (for now; typed methods to follow).
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Insert a freshly submitted bundle.
    ///
    /// TODO: implement the INSERT against `bundle_submissions`.
    pub async fn record_submission(&self, _lc: &BundleLifecycle) -> anyhow::Result<()> {
        // TODO: persist submission row.
        todo!("record_submission not implemented yet")
    }

    /// Advance a bundle to a later commitment stage and recompute deltas.
    ///
    /// TODO: implement the UPDATE + latency-delta computation.
    pub async fn record_stage(&self, _bundle_id: &str) -> anyhow::Result<()> {
        // TODO: update stage timestamp + inter-stage deltas.
        todo!("record_stage not implemented yet")
    }
}
