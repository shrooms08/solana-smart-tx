//! Jito leader-window detection (Plan B).
//!
//! The pinned `jito-sdk-rust` v0.3.2 exposes only bundle / tip / transaction
//! endpoints — no `getNextScheduledLeader` / `getConnectedLeaders` — so leader
//! detection is built here from two independent data sources:
//!
//!   * **Leader schedule** — `solana-client` (nonblocking) `getSlotLeaders`,
//!     fetched for a lookahead window and refreshed periodically.
//!   * **Jito validator set** — Jito's public validator API
//!     (`https://kobe.mainnet.jito.network/api/v1/validators`), filtered to
//!     `running_jito == true`, keyed by `identity_account`. Cached with a TTL.
//!
//! The **slot clock** comes exclusively from the live stream (`processed` slot
//! updates ingested via [`LeaderTracker::ingest`]) — never from polling
//! `getSlot`. RPC/HTTP are only ever used for schedule + Jito-set data.
//!
//! The next Jito leader slot is the first upcoming slot in the cached schedule
//! whose leader pubkey is in the Jito set; the window math lives in the pure,
//! unit-tested [`next_jito_leader`].

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use solana_sdk::pubkey::Pubkey;
use stream::{SlotStatus, StreamEvent};
use tracing::{debug, info, warn};

/// Jito's public validator API (mainnet). Each entry carries `identity_account`
/// (the leader/node identity, which is what `getSlotLeaders` returns) and a
/// `running_jito` boolean.
pub const DEFAULT_JITO_VALIDATORS_URL: &str =
    "https://kobe.mainnet.jito.network/api/v1/validators";

// ---------------------------------------------------------------------------
// Public config / output types
// ---------------------------------------------------------------------------

/// Tuning for the tracker.
#[derive(Debug, Clone)]
pub struct LeaderConfig {
    /// Fire [`LeaderTracker::wait_for_window`] once `slots_until <= this`.
    pub threshold_slots: u64,
    /// How many slots of leader schedule to fetch per refresh.
    pub lookahead_slots: u64,
    /// How often to re-fetch the leader schedule (re-anchored to the clock).
    pub schedule_refresh: Duration,
    /// How often to re-fetch the Jito validator set.
    pub jito_refresh: Duration,
    /// Max age of the slot clock before it's considered stale (a health error).
    pub max_clock_staleness: Duration,
    /// Max age of the cached schedule before it's considered stale.
    pub max_schedule_age: Duration,
    /// Max age of the cached Jito set before it's considered stale.
    pub max_jito_age: Duration,
}

impl Default for LeaderConfig {
    fn default() -> Self {
        Self {
            threshold_slots: 2,
            lookahead_slots: 1000,
            schedule_refresh: Duration::from_secs(10),
            jito_refresh: Duration::from_secs(600), // 10 min
            max_clock_staleness: Duration::from_secs(2),
            // Generous grace over the refresh intervals so a single failed
            // refresh doesn't flap the health state, but a persistent failure
            // (stale data) still surfaces.
            max_schedule_age: Duration::from_secs(30),
            max_jito_age: Duration::from_secs(1200), // 20 min
        }
    }
}

/// Answer to "when is the next Jito leader slot, and how close are we?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderWindow {
    /// Current slot per the live (processed) slot clock.
    pub current_slot: u64,
    /// First upcoming slot (>= `current_slot`) whose leader runs Jito.
    pub next_jito_leader_slot: u64,
    /// `next_jito_leader_slot - current_slot`. Zero when the *current* leader
    /// is already a Jito leader (mid-window).
    pub slots_until: u64,
    /// Base58 identity of that leader, if known.
    pub leader_identity: Option<String>,
}

/// Health / staleness states surfaced as errors rather than stale windows.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LeaderError {
    #[error("slot clock has not received any processed slot yet")]
    NoClock,
    #[error("slot clock stale: no processed slot update within the staleness window")]
    ClockStale,
    #[error("leader schedule not fetched yet")]
    NoSchedule,
    #[error("leader schedule is stale (older than max_schedule_age)")]
    ScheduleStale,
    #[error("slot clock advanced past the cached leader schedule window")]
    ScheduleExhausted,
    #[error("Jito validator set not fetched yet")]
    NoJitoData,
    #[error("Jito validator set is stale (older than max_jito_age)")]
    JitoStale,
    #[error("no Jito leader found within the lookahead window")]
    NoJitoLeaderInLookahead,
}

// ---------------------------------------------------------------------------
// Data source trait + production implementation
// ---------------------------------------------------------------------------

/// The two network-backed inputs the tracker needs. Abstracted so unit tests
/// can mock them without touching the network.
pub trait LeaderDataSource: Send + Sync + 'static {
    /// Upcoming leader identities for `[start_slot, start_slot + limit)`.
    /// `result[i]` is the leader of `start_slot + i`.
    fn fetch_slot_leaders(
        &self,
        start_slot: u64,
        limit: u64,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<Pubkey>>> + Send;

    /// The set of validator identities currently running Jito.
    fn fetch_jito_validators(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<HashSet<Pubkey>>> + Send;
}

/// Production data source: `solana-client` for the schedule, reqwest for the
/// Jito validator set.
pub struct RpcJitoSource {
    rpc: solana_client::nonblocking::rpc_client::RpcClient,
    http: reqwest::Client,
    jito_validators_url: String,
}

impl RpcJitoSource {
    /// Construct with an explicit Jito validator API URL.
    pub fn new(rpc_url: String, jito_validators_url: String) -> Self {
        Self {
            rpc: solana_client::nonblocking::rpc_client::RpcClient::new(rpc_url),
            http: reqwest::Client::new(),
            jito_validators_url,
        }
    }

    /// Construct against the default mainnet Jito validator API.
    pub fn mainnet(rpc_url: String) -> Self {
        Self::new(rpc_url, DEFAULT_JITO_VALIDATORS_URL.to_string())
    }
}

/// Shape of the kobe `/api/v1/validators` response we care about.
#[derive(Debug, serde::Deserialize)]
struct KobeResponse {
    validators: Vec<KobeValidator>,
}

#[derive(Debug, serde::Deserialize)]
struct KobeValidator {
    identity_account: String,
    #[serde(default)]
    running_jito: bool,
}

// The trait declares `-> impl Future + Send` to guarantee `Send` futures for
// `tokio::spawn`; an `async fn` in the impl satisfies that requirement and is
// only compile-accepted if the resulting future is in fact `Send`.
impl LeaderDataSource for RpcJitoSource {
    async fn fetch_slot_leaders(
        &self,
        start_slot: u64,
        limit: u64,
    ) -> anyhow::Result<Vec<Pubkey>> {
        let leaders = self.rpc.get_slot_leaders(start_slot, limit).await?;
        Ok(leaders)
    }

    async fn fetch_jito_validators(&self) -> anyhow::Result<HashSet<Pubkey>> {
        let resp: KobeResponse = self
            .http
            .get(&self.jito_validators_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let set = resp
            .validators
            .into_iter()
            .filter(|v| v.running_jito)
            .filter_map(|v| Pubkey::from_str(&v.identity_account).ok())
            .collect();
        Ok(set)
    }
}

// ---------------------------------------------------------------------------
// Internal cached state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct SlotClock {
    /// Highest processed slot seen so far.
    slot: Option<u64>,
    /// When the clock last ticked (monotonic).
    updated_at: Option<Instant>,
}

struct LeaderSchedule {
    start_slot: u64,
    /// `leaders[i]` is the leader of `start_slot + i`.
    leaders: Vec<Pubkey>,
    fetched_at: Instant,
}

struct JitoSet {
    set: HashSet<Pubkey>,
    fetched_at: Instant,
}

struct Inner<D> {
    config: LeaderConfig,
    source: D,
    clock: Mutex<SlotClock>,
    schedule: RwLock<Option<LeaderSchedule>>,
    jito: RwLock<Option<JitoSet>>,
    /// Notified on every processed slot tick so `wait_for_window` can re-check.
    tick: tokio::sync::Notify,
}

impl<D: LeaderDataSource> Inner<D> {
    async fn refresh_jito(&self) -> anyhow::Result<()> {
        let set = self.source.fetch_jito_validators().await?;
        let n = set.len();
        *self.jito.write().unwrap() = Some(JitoSet {
            set,
            fetched_at: Instant::now(),
        });
        info!(jito_validators = n, "refreshed Jito validator set");
        Ok(())
    }

    async fn refresh_schedule(&self) -> anyhow::Result<()> {
        let start = self
            .clock
            .lock()
            .unwrap()
            .slot
            .ok_or(LeaderError::NoClock)?;
        let leaders = self
            .source
            .fetch_slot_leaders(start, self.config.lookahead_slots)
            .await?;
        let len = leaders.len();
        *self.schedule.write().unwrap() = Some(LeaderSchedule {
            start_slot: start,
            leaders,
            fetched_at: Instant::now(),
        });
        info!(start_slot = start, leaders = len, "refreshed leader schedule");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Pure window math (unit-tested)
// ---------------------------------------------------------------------------

/// First slot `>= current_slot` within the cached schedule whose leader is in
/// `jito`, plus that leader. `None` if the lookahead window contains no Jito
/// leader. Returns `current_slot` itself when the current leader runs Jito.
fn next_jito_leader(
    current_slot: u64,
    start_slot: u64,
    leaders: &[Pubkey],
    jito: &HashSet<Pubkey>,
) -> Option<(u64, Pubkey)> {
    if leaders.is_empty() {
        return None;
    }
    let end = start_slot + leaders.len() as u64; // exclusive
    let from = current_slot.max(start_slot);
    let mut slot = from;
    while slot < end {
        let idx = (slot - start_slot) as usize;
        let leader = leaders[idx];
        if jito.contains(&leader) {
            return Some((slot, leader));
        }
        slot += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Tracker
// ---------------------------------------------------------------------------

/// Tracks the live slot clock + leader schedule + Jito set and answers
/// "are we approaching a Jito leader window?".
///
/// Cheap to [`Clone`] (an `Arc` bump); share one instance between the ingest
/// task, the refresh loops, and query sites.
pub struct LeaderTracker<D> {
    inner: Arc<Inner<D>>,
}

impl<D> Clone for LeaderTracker<D> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<D: LeaderDataSource> LeaderTracker<D> {
    /// Construct a tracker. Does no I/O and spawns nothing; call
    /// [`LeaderTracker::spawn_refresh`] to start the background refresh loops,
    /// and feed the clock via [`LeaderTracker::ingest`].
    pub fn new(config: LeaderConfig, source: D) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                source,
                clock: Mutex::new(SlotClock::default()),
                schedule: RwLock::new(None),
                jito: RwLock::new(None),
                tick: tokio::sync::Notify::new(),
            }),
        }
    }

    /// Ingest one stream event. Only `Slot` events with `processed` status
    /// advance the clock; everything else is ignored. Sync and cheap — call it
    /// straight from the stream receive loop.
    pub fn ingest(&self, event: &StreamEvent) {
        let StreamEvent::Slot(slot) = event else {
            return;
        };
        if slot.status != SlotStatus::Processed {
            return;
        }
        {
            let mut clock = self.inner.clock.lock().unwrap();
            // Monotonic: never move the clock backwards on a late/duplicate slot,
            // but always record liveness.
            clock.slot = Some(clock.slot.map_or(slot.slot, |s| s.max(slot.slot)));
            clock.updated_at = Some(Instant::now());
        }
        self.inner.tick.notify_waiters();
    }

    /// Drain a `StreamEvent` receiver into the clock until it closes. Convenience
    /// wrapper over [`LeaderTracker::ingest`]; spawn it on its own task.
    pub async fn ingest_loop(self, mut rx: tokio::sync::mpsc::Receiver<StreamEvent>) {
        while let Some(event) = rx.recv().await {
            self.ingest(&event);
        }
        debug!("stream event channel closed; leader ingest loop stopping");
    }

    /// Force a one-shot refresh of the Jito validator set.
    pub async fn refresh_jito(&self) -> anyhow::Result<()> {
        self.inner.refresh_jito().await
    }

    /// Force a one-shot refresh of the leader schedule (requires a live clock).
    pub async fn refresh_schedule(&self) -> anyhow::Result<()> {
        self.inner.refresh_schedule().await
    }

    /// Spawn the background refresh loops (schedule + Jito set). The loops hold a
    /// [`Weak`] reference and self-terminate once every [`LeaderTracker`] clone
    /// has been dropped.
    pub fn spawn_refresh(&self) {
        tokio::spawn(Self::jito_refresh_loop(Arc::downgrade(&self.inner)));
        tokio::spawn(Self::schedule_refresh_loop(Arc::downgrade(&self.inner)));
    }

    async fn jito_refresh_loop(weak: Weak<Inner<D>>) {
        loop {
            let inner = match weak.upgrade() {
                Some(inner) => inner,
                None => break,
            };
            let interval = inner.config.jito_refresh;
            if let Err(err) = inner.refresh_jito().await {
                warn!(error = %err, "failed to refresh Jito validator set");
            }
            drop(inner); // don't hold a strong ref across the sleep
            tokio::time::sleep(interval).await;
        }
    }

    async fn schedule_refresh_loop(weak: Weak<Inner<D>>) {
        loop {
            let inner = match weak.upgrade() {
                Some(inner) => inner,
                None => break,
            };
            let has_clock = inner.clock.lock().unwrap().slot.is_some();
            let interval = if has_clock {
                inner.config.schedule_refresh
            } else {
                // No slot clock yet — poll quickly until the stream warms up.
                Duration::from_millis(500)
            };
            if has_clock {
                if let Err(err) = inner.refresh_schedule().await {
                    warn!(error = %err, "failed to refresh leader schedule");
                }
            }
            drop(inner);
            tokio::time::sleep(interval).await;
        }
    }

    /// Compute the current Jito-leader window, or a [`LeaderError`] health state
    /// if any input is missing or stale.
    pub async fn current_window(&self) -> anyhow::Result<LeaderWindow> {
        let inner = &self.inner;

        // --- slot clock (from the live stream) ---
        let current_slot = {
            let clock = inner.clock.lock().unwrap();
            let slot = clock.slot.ok_or(LeaderError::NoClock)?;
            let updated_at = clock.updated_at.ok_or(LeaderError::NoClock)?;
            if updated_at.elapsed() > inner.config.max_clock_staleness {
                return Err(LeaderError::ClockStale.into());
            }
            slot
        };

        // --- leader schedule (RPC, cached) ---
        let schedule_guard = inner.schedule.read().unwrap();
        let schedule = schedule_guard.as_ref().ok_or(LeaderError::NoSchedule)?;
        if schedule.fetched_at.elapsed() > inner.config.max_schedule_age {
            return Err(LeaderError::ScheduleStale.into());
        }
        if current_slot >= schedule.start_slot + schedule.leaders.len() as u64 {
            return Err(LeaderError::ScheduleExhausted.into());
        }

        // --- Jito set (HTTP, cached) ---
        let jito_guard = inner.jito.read().unwrap();
        let jito = jito_guard.as_ref().ok_or(LeaderError::NoJitoData)?;
        if jito.fetched_at.elapsed() > inner.config.max_jito_age {
            return Err(LeaderError::JitoStale.into());
        }

        let (next_slot, leader) = next_jito_leader(
            current_slot,
            schedule.start_slot,
            &schedule.leaders,
            &jito.set,
        )
        .ok_or(LeaderError::NoJitoLeaderInLookahead)?;

        Ok(LeaderWindow {
            current_slot,
            next_jito_leader_slot: next_slot,
            slots_until: next_slot - current_slot,
            leader_identity: Some(leader.to_string()),
        })
    }

    /// Resolve once the next Jito leader slot is within `threshold_slots`.
    ///
    /// Re-checks on every slot tick (and at least every 250ms). Surfaces a
    /// [`LeaderError`] health state as an error rather than blocking on stale
    /// data.
    pub async fn wait_for_window(&self) -> anyhow::Result<LeaderWindow> {
        loop {
            // Arm the tick notification *before* checking, so we can't miss a
            // tick that lands between the check and the await.
            let notified = self.inner.tick.notified();

            match self.current_window().await {
                Ok(window) if window.slots_until <= self.inner.config.threshold_slots => {
                    info!(
                        current_slot = window.current_slot,
                        next_jito_leader_slot = window.next_jito_leader_slot,
                        slots_until = window.slots_until,
                        leader = ?window.leader_identity,
                        "Jito leader window open"
                    );
                    return Ok(window);
                }
                Ok(_) => {}
                Err(err) => return Err(err),
            }

            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// These cover the pure window math and the tracker's window/staleness logic
// against a mocked data source — no network. Live end-to-end verification is the
// `leader_probe` example against real infra.
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use stream::SlotUpdate;

    fn jito_set(keys: &[Pubkey]) -> HashSet<Pubkey> {
        keys.iter().copied().collect()
    }

    fn slot_event(slot: u64) -> StreamEvent {
        StreamEvent::Slot(SlotUpdate {
            slot,
            status: SlotStatus::Processed,
            received_at: SystemTime::now(),
        })
    }

    // --- pure window math ---

    #[test]
    fn next_jito_leader_finds_first_upcoming() {
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        // slots 100..=103 -> a (non-Jito), 104..=107 -> b (Jito)
        let leaders = vec![a, a, a, a, b, b, b, b];
        let jito = jito_set(&[b]);
        let (slot, leader) = next_jito_leader(100, 100, &leaders, &jito).unwrap();
        assert_eq!(slot, 104);
        assert_eq!(leader, b);
    }

    #[test]
    fn next_jito_leader_zero_when_current_is_jito() {
        let a = Pubkey::new_unique();
        let leaders = vec![a, a, a, a];
        let jito = jito_set(&[a]);
        // Current leader IS a Jito leader -> next slot is the current slot.
        let (slot, _) = next_jito_leader(101, 100, &leaders, &jito).unwrap();
        assert_eq!(slot, 101);
    }

    #[test]
    fn next_jito_leader_none_when_no_jito_in_window() {
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        let leaders = vec![a, a, b, b];
        let jito = jito_set(&[Pubkey::new_unique()]); // some other validator
        assert!(next_jito_leader(100, 100, &leaders, &jito).is_none());
    }

    // --- tracker against a mock source ---

    struct MockSource {
        leaders: Vec<Pubkey>,
        jito: HashSet<Pubkey>,
    }

    impl LeaderDataSource for MockSource {
        async fn fetch_slot_leaders(
            &self,
            _start_slot: u64,
            _limit: u64,
        ) -> anyhow::Result<Vec<Pubkey>> {
            Ok(self.leaders.clone())
        }

        async fn fetch_jito_validators(&self) -> anyhow::Result<HashSet<Pubkey>> {
            Ok(self.jito.clone())
        }
    }

    fn downcast(err: anyhow::Error) -> LeaderError {
        err.downcast::<LeaderError>().expect("expected LeaderError")
    }

    #[tokio::test]
    async fn window_via_mock_source() {
        let a = Pubkey::new_unique();
        let b = Pubkey::new_unique();
        let leaders = vec![a, a, a, a, b, b, b, b]; // anchored at slot 100
        let source = MockSource {
            leaders,
            jito: jito_set(&[b]),
        };
        let tracker = LeaderTracker::new(LeaderConfig::default(), source);

        // No clock yet -> NoClock.
        assert_eq!(
            downcast(tracker.current_window().await.unwrap_err()),
            LeaderError::NoClock
        );

        tracker.ingest(&slot_event(100));
        // No schedule / jito yet -> NoSchedule.
        assert_eq!(
            downcast(tracker.current_window().await.unwrap_err()),
            LeaderError::NoSchedule
        );

        tracker.refresh_jito().await.unwrap();
        tracker.refresh_schedule().await.unwrap();

        let window = tracker.current_window().await.unwrap();
        assert_eq!(window.current_slot, 100);
        assert_eq!(window.next_jito_leader_slot, 104);
        assert_eq!(window.slots_until, 4);
        assert_eq!(window.leader_identity, Some(b.to_string()));
    }

    #[tokio::test]
    async fn current_leader_is_jito_gives_zero_slots_until() {
        let a = Pubkey::new_unique();
        let source = MockSource {
            leaders: vec![a, a, a, a],
            jito: jito_set(&[a]),
        };
        let tracker = LeaderTracker::new(LeaderConfig::default(), source);
        tracker.ingest(&slot_event(100));
        tracker.refresh_jito().await.unwrap();
        tracker.refresh_schedule().await.unwrap();

        let window = tracker.current_window().await.unwrap();
        assert_eq!(window.slots_until, 0);
        assert_eq!(window.next_jito_leader_slot, 100);
    }

    #[tokio::test]
    async fn stale_clock_surfaces_as_error() {
        let a = Pubkey::new_unique();
        let config = LeaderConfig {
            max_clock_staleness: Duration::ZERO, // any elapsed time is "stale"
            ..Default::default()
        };
        let source = MockSource {
            leaders: vec![a; 4],
            jito: jito_set(&[a]),
        };
        let tracker = LeaderTracker::new(config, source);
        tracker.ingest(&slot_event(100));
        tracker.refresh_jito().await.unwrap();
        tracker.refresh_schedule().await.unwrap();

        // Let a moment pass so updated_at.elapsed() > 0.
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(
            downcast(tracker.current_window().await.unwrap_err()),
            LeaderError::ClockStale
        );
    }

    #[tokio::test]
    async fn no_jito_leader_in_lookahead_is_error() {
        let a = Pubkey::new_unique();
        let source = MockSource {
            leaders: vec![a; 4],
            jito: jito_set(&[Pubkey::new_unique()]), // a is not Jito
        };
        let tracker = LeaderTracker::new(LeaderConfig::default(), source);
        tracker.ingest(&slot_event(100));
        tracker.refresh_jito().await.unwrap();
        tracker.refresh_schedule().await.unwrap();

        assert_eq!(
            downcast(tracker.current_window().await.unwrap_err()),
            LeaderError::NoJitoLeaderInLookahead
        );
    }

    #[tokio::test]
    async fn clock_is_monotonic() {
        let a = Pubkey::new_unique();
        let source = MockSource {
            leaders: vec![a; 4],
            jito: jito_set(&[a]),
        };
        let tracker = LeaderTracker::new(LeaderConfig::default(), source);
        tracker.ingest(&slot_event(105));
        tracker.ingest(&slot_event(103)); // late/out-of-order, must not regress
        tracker.refresh_jito().await.unwrap();
        // schedule anchored at the (monotonic) clock = 105
        tracker.refresh_schedule().await.unwrap();
        let window = tracker.current_window().await.unwrap();
        assert_eq!(window.current_slot, 105);
    }
}
