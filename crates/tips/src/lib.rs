//! Jito bundle-tip market-data feed.
//!
//! Maintains the current tip percentiles (in **lamports**) plus a short rolling
//! history, sourced from Jito's public tip feeds:
//!
//!   * **WebSocket (primary):** `wss://bundles.jito.wtf/api/v1/bundles/tip_stream`
//!   * **REST (fallback):** `https://bundles.jito.wtf/api/v1/bundles/tip_floor`
//!
//! Both deliver the same rolling tip stats. SOL-denominated floats are converted
//! to lamports (round half-up) at the ingest boundary — **no floats leave this
//! crate**.
//!
//! # This crate is a FEED, not a policy
//!
//! [`TipTracker`] exposes the latest snapshot, its freshness, a staleness flag,
//! and a trend. It deliberately exposes **no recommendation / "what should I
//! tip?" method**. Tipping decisions belong to the submitter / agent layer; this
//! crate only reports observed market data.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tracing::{debug, info, warn};

/// Working tip_stream WebSocket URL (verified live; see crate docs).
pub const TIP_STREAM_WS_URL: &str = "wss://bundles.jito.wtf/api/v1/bundles/tip_stream";
/// Working tip_floor REST URL (verified live).
pub const TIP_FLOOR_REST_URL: &str = "https://bundles.jito.wtf/api/v1/bundles/tip_floor";

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

// ---------------------------------------------------------------------------
// SOL -> lamports (the only float boundary)
// ---------------------------------------------------------------------------

/// Convert a SOL-denominated float to lamports, rounding half-up.
///
/// Negative / non-finite inputs clamp to `0`. `f64::round` rounds halves away
/// from zero, which for these non-negative tip values is exactly round-half-up.
/// This is the *only* place SOL floats cross into the crate; everything the
/// crate exposes is integer lamports.
pub fn sol_to_lamports(sol: f64) -> u64 {
    if !sol.is_finite() || sol <= 0.0 {
        return 0;
    }
    (sol * LAMPORTS_PER_SOL).round() as u64
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A point-in-time view of Jito's landed-tip percentiles, in lamports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipSnapshot {
    /// Local monotonic time the snapshot was ingested (used for freshness).
    pub taken_at: Instant,
    pub p25_lamports: u64,
    pub p50_lamports: u64,
    pub p75_lamports: u64,
    pub p95_lamports: u64,
    pub p99_lamports: u64,
    /// EMA of the 50th-percentile landed tip.
    pub ema_p50_lamports: u64,
}

/// Which feed a snapshot came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipSource {
    WebSocket,
    Rest,
}

impl fmt::Display for TipSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TipSource::WebSocket => f.write_str("websocket"),
            TipSource::Rest => f.write_str("rest"),
        }
    }
}

/// Change in tip percentiles over a trend window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipTrend {
    /// `latest.p50 - oldest_in_window.p50` (signed lamports).
    pub p50_change_lamports: i64,
    /// `latest.p75 - oldest_in_window.p75` (signed lamports).
    pub p75_change_lamports: i64,
    /// Whether the 50th percentile rose over the window.
    pub rising: bool,
}

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

/// Raw row as delivered by both the WebSocket and REST feeds. Both send a JSON
/// array of these (typically length 1).
#[derive(Debug, Clone, Deserialize)]
struct RawTipFloor {
    /// ISO-8601 server timestamp (kept for logging/debug; not used for freshness).
    #[serde(default)]
    #[allow(dead_code)]
    time: Option<String>,
    landed_tips_25th_percentile: f64,
    landed_tips_50th_percentile: f64,
    landed_tips_75th_percentile: f64,
    landed_tips_95th_percentile: f64,
    landed_tips_99th_percentile: f64,
    ema_landed_tips_50th_percentile: f64,
}

impl RawTipFloor {
    fn into_snapshot(self, taken_at: Instant) -> TipSnapshot {
        TipSnapshot {
            taken_at,
            p25_lamports: sol_to_lamports(self.landed_tips_25th_percentile),
            p50_lamports: sol_to_lamports(self.landed_tips_50th_percentile),
            p75_lamports: sol_to_lamports(self.landed_tips_75th_percentile),
            p95_lamports: sol_to_lamports(self.landed_tips_95th_percentile),
            p99_lamports: sol_to_lamports(self.landed_tips_99th_percentile),
            ema_p50_lamports: sol_to_lamports(self.ema_landed_tips_50th_percentile),
        }
    }
}

/// Parse a raw feed payload (JSON array) into a snapshot, stamped `now`.
fn parse_payload(text: &str, taken_at: Instant) -> anyhow::Result<TipSnapshot> {
    let rows: Vec<RawTipFloor> = serde_json::from_str(text)?;
    // The feed sends a 1-element array; take the most recent row if more.
    let row = rows
        .into_iter()
        .next_back()
        .ok_or_else(|| anyhow::anyhow!("empty tip payload"))?;
    Ok(row.into_snapshot(taken_at))
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Tuning for [`TipTracker`].
#[derive(Debug, Clone)]
pub struct TipConfig {
    /// Max snapshots retained for trend computation.
    pub history_capacity: usize,
    /// Age beyond which [`TipTracker::is_stale`] returns `true`.
    pub staleness_threshold: Duration,
    /// REST poll cadence while the WebSocket is down.
    pub rest_poll_interval: Duration,
    /// Backoff base for WebSocket reconnects.
    pub backoff_base: Duration,
    /// Backoff cap for WebSocket reconnects.
    pub backoff_cap: Duration,
    /// A connection up at least this long resets the backoff schedule.
    pub healthy_reset: Duration,
}

impl Default for TipConfig {
    fn default() -> Self {
        Self {
            history_capacity: 120,
            staleness_threshold: Duration::from_secs(30),
            rest_poll_interval: Duration::from_secs(5),
            backoff_base: Duration::from_millis(250),
            backoff_cap: Duration::from_secs(30),
            healthy_reset: Duration::from_secs(10),
        }
    }
}

// ---------------------------------------------------------------------------
// Reconnect backoff (same pattern as the stream crate)
// ---------------------------------------------------------------------------

/// Exponential backoff with full jitter: the delay is a uniform draw in
/// `(0, ceiling]` where `ceiling = min(cap, base * 2^attempt)`.
#[derive(Debug, Clone)]
struct Backoff {
    base: Duration,
    cap: Duration,
    attempt: u32,
}

impl Backoff {
    fn new(base: Duration, cap: Duration) -> Self {
        Self {
            base,
            cap,
            attempt: 0,
        }
    }

    fn reset(&mut self) {
        self.attempt = 0;
    }

    fn ceiling(&self) -> Duration {
        let scaled = self.base.as_secs_f64() * 2.0_f64.powi(self.attempt as i32);
        Duration::from_secs_f64(scaled.min(self.cap.as_secs_f64()))
    }

    fn next_delay(&mut self) -> Duration {
        let ceiling = self.ceiling();
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_secs_f64(ceiling.as_secs_f64() * rand::random::<f64>())
    }
}

// ---------------------------------------------------------------------------
// Transport trait + live implementation
// ---------------------------------------------------------------------------

/// The two feeds the tracker consumes, abstracted so tests run offline.
pub trait TipTransport: Send + Sync + 'static {
    /// Drive the WebSocket, pushing each raw text frame into `sink` until the
    /// connection ends. Returns `Ok(())` on clean close, `Err` on failure.
    fn run_websocket(
        &self,
        sink: tokio::sync::mpsc::Sender<String>,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;

    /// One-shot REST poll returning the raw JSON body.
    fn fetch_rest(&self) -> impl std::future::Future<Output = anyhow::Result<String>> + Send;
}

/// Live transport: tokio-tungstenite (rustls) for the stream, reqwest for REST.
pub struct LiveTransport {
    http: reqwest::Client,
    ws_url: String,
    rest_url: String,
}

impl LiveTransport {
    pub fn new() -> Self {
        Self::with_urls(TIP_STREAM_WS_URL.to_string(), TIP_FLOOR_REST_URL.to_string())
    }

    /// Construct with explicit feed URLs (handy for pointing at a staging host,
    /// or — in the probe — at a dead WS URL to exercise the REST fallback live).
    pub fn with_urls(ws_url: String, rest_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            ws_url,
            rest_url,
        }
    }
}

impl Default for LiveTransport {
    fn default() -> Self {
        Self::new()
    }
}

// The trait declares `-> impl Future + Send`; an `async fn` here satisfies it
// and is only accepted if the resulting future is actually `Send`.
impl TipTransport for LiveTransport {
    async fn run_websocket(
        &self,
        sink: tokio::sync::mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        use tokio_tungstenite::tungstenite::Message;

        // Ensure the rustls CryptoProvider is installed (shared, process-wide).
        runtime::init_crypto();
        let (ws_stream, _resp) = tokio_tungstenite::connect_async(&self.ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        while let Some(msg) = read.next().await {
            match msg? {
                Message::Text(text) => {
                    if sink.send(text.to_string()).await.is_err() {
                        // Consumer gone; stop reading.
                        break;
                    }
                }
                Message::Ping(payload) => {
                    // Keep the connection alive (we otherwise only read).
                    let _ = write.send(Message::Pong(payload)).await;
                }
                Message::Close(_) => break,
                Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
            }
        }
        Ok(())
    }

    async fn fetch_rest(&self) -> anyhow::Result<String> {
        let body = self
            .http
            .get(&self.rest_url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(body)
    }
}

// ---------------------------------------------------------------------------
// Tracker
// ---------------------------------------------------------------------------

#[derive(Default)]
struct State {
    history: VecDeque<TipSnapshot>,
    last_source: Option<TipSource>,
}

struct Inner<T> {
    config: TipConfig,
    transport: T,
    state: Mutex<State>,
}

/// Maintains the latest tip percentiles + a bounded history, fed by the Jito
/// tip feeds. Cheap to [`Clone`] (an `Arc` bump).
///
/// **This is a feed, not a policy.** There is intentionally no method that
/// recommends a tip; callers read [`TipTracker::latest`] / [`TipTracker::trend`]
/// / [`TipTracker::is_stale`] and decide for themselves.
pub struct TipTracker<T = LiveTransport> {
    inner: Arc<Inner<T>>,
}

impl<T> Clone for TipTracker<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl TipTracker<LiveTransport> {
    /// Construct a tracker against the live Jito feeds.
    pub fn live(config: TipConfig) -> Self {
        Self::new(config, LiveTransport::new())
    }
}

impl<T: TipTransport> TipTracker<T> {
    /// Construct a tracker over an arbitrary transport (used by tests).
    pub fn new(config: TipConfig, transport: T) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                transport,
                state: Mutex::new(State::default()),
            }),
        }
    }

    /// Most recent snapshot, if any.
    pub fn latest(&self) -> Option<TipSnapshot> {
        self.inner.state.lock().unwrap().history.back().copied()
    }

    /// Which feed the most recent snapshot came from.
    pub fn latest_source(&self) -> Option<TipSource> {
        self.inner.state.lock().unwrap().last_source
    }

    /// Age of the latest snapshot, if any.
    pub fn freshness(&self) -> Option<Duration> {
        self.inner
            .state
            .lock()
            .unwrap()
            .history
            .back()
            .map(|s| s.taken_at.elapsed())
    }

    /// Whether the latest data is older than the staleness threshold (or absent).
    /// The orchestrator/agent should check this before trusting the feed.
    pub fn is_stale(&self) -> bool {
        match self.freshness() {
            Some(age) => age > self.inner.config.staleness_threshold,
            None => true,
        }
    }

    /// Trend over `window`: compares the oldest snapshot within the window to the
    /// latest. `None` if there are fewer than two snapshots in the window.
    pub fn trend(&self, window: Duration) -> Option<TipTrend> {
        let state = self.inner.state.lock().unwrap();
        compute_trend(&state.history, Instant::now(), window)
    }

    fn store(&self, snapshot: TipSnapshot, source: TipSource) {
        let mut state = self.inner.state.lock().unwrap();
        state.history.push_back(snapshot);
        while state.history.len() > self.inner.config.history_capacity {
            state.history.pop_front();
        }
        state.last_source = Some(source);
    }

    fn ingest(&self, text: &str, source: TipSource) {
        match parse_payload(text, Instant::now()) {
            Ok(snapshot) => {
                debug!(
                    %source,
                    p50_lamports = snapshot.p50_lamports,
                    p75_lamports = snapshot.p75_lamports,
                    "ingested tip snapshot"
                );
                self.store(snapshot, source);
            }
            Err(err) => warn!(%source, error = %err, "failed to parse tip payload"),
        }
    }

    /// Supervisor: keep tip data flowing forever.
    ///
    /// Primary path is the WebSocket. On socket failure it backs off
    /// exponentially (with jitter) and, *while down*, polls the REST endpoint so
    /// data keeps flowing; it returns to the socket once it recovers.
    pub async fn run(&self) -> anyhow::Result<()> {
        let mut backoff =
            Backoff::new(self.inner.config.backoff_base, self.inner.config.backoff_cap);

        loop {
            info!(url = %runtime::redact_url(TIP_STREAM_WS_URL), "connecting tip websocket");
            let connected_at = Instant::now();
            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
            let ws = self.inner.transport.run_websocket(tx);
            tokio::pin!(ws);

            let mut got_any = false;
            let ws_result: anyhow::Result<()> = loop {
                tokio::select! {
                    res = &mut ws => break res,
                    maybe = rx.recv() => match maybe {
                        Some(text) => {
                            got_any = true;
                            self.ingest(&text, TipSource::WebSocket);
                        }
                        None => break Ok(()),
                    },
                }
            };

            match ws_result {
                Ok(()) => warn!(uptime = ?connected_at.elapsed(), "tip websocket closed"),
                Err(err) => {
                    warn!(uptime = ?connected_at.elapsed(), error = %err, "tip websocket error")
                }
            }

            // Reset the schedule only after a healthy connection, so a flapping
            // socket doesn't tight-loop.
            if got_any && connected_at.elapsed() >= self.inner.config.healthy_reset {
                backoff.reset();
            }

            let delay = backoff.next_delay();
            warn!(?delay, "tip websocket down; polling REST during backoff");
            self.rest_poll_during(delay).await;
        }
    }

    /// Poll the REST endpoint every `rest_poll_interval` until `delay` elapses,
    /// keeping data flowing while the socket is down.
    async fn rest_poll_during(&self, delay: Duration) {
        let deadline = Instant::now() + delay;
        loop {
            match self.inner.transport.fetch_rest().await {
                Ok(text) => self.ingest(&text, TipSource::Rest),
                Err(err) => warn!(error = %err, "REST tip poll failed"),
            }
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let nap = (deadline - now).min(self.inner.config.rest_poll_interval);
            tokio::time::sleep(nap).await;
        }
    }
}

/// Pure trend math over an ordered (oldest -> newest) history.
fn compute_trend(
    history: &VecDeque<TipSnapshot>,
    now: Instant,
    window: Duration,
) -> Option<TipTrend> {
    let latest = *history.back()?;
    let oldest = *history
        .iter()
        .find(|s| now.saturating_duration_since(s.taken_at) <= window)?;
    if oldest.taken_at == latest.taken_at {
        // Only one snapshot falls within the window.
        return None;
    }
    let p50_change = latest.p50_lamports as i64 - oldest.p50_lamports as i64;
    let p75_change = latest.p75_lamports as i64 - oldest.p75_lamports as i64;
    Some(TipTrend {
        p50_change_lamports: p50_change,
        p75_change_lamports: p75_change,
        rising: p50_change > 0,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Pure logic (conversion, trend, staleness) plus a mocked transport so the
// suite runs offline. Live verification is the `tip_probe` example.
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[{"time":"2026-06-10T12:33:05+00:00","landed_tips_25th_percentile":1.221e-6,"landed_tips_50th_percentile":2.4050000000000003e-6,"landed_tips_75th_percentile":5e-6,"landed_tips_95th_percentile":0.000025409599999999754,"landed_tips_99th_percentile":0.00009369504000000003,"ema_landed_tips_50th_percentile":2.6075112899878422e-6}]"#;

    // --- SOL -> lamports ---

    #[test]
    fn sol_to_lamports_basic() {
        assert_eq!(sol_to_lamports(1.0), 1_000_000_000);
        assert_eq!(sol_to_lamports(0.0), 0);
        assert_eq!(sol_to_lamports(5e-6), 5_000); // 5 micro-SOL = 5000 lamports
    }

    #[test]
    fn sol_to_lamports_rounds_half_up() {
        // 1.5 lamports -> 2, 2.5 -> 3, 0.5 -> 1 (round half away from zero).
        assert_eq!(sol_to_lamports(1.5e-9), 2);
        assert_eq!(sol_to_lamports(2.5e-9), 3);
        assert_eq!(sol_to_lamports(0.5e-9), 1);
        // Just under a half rounds down.
        assert_eq!(sol_to_lamports(1.4e-9), 1);
    }

    #[test]
    fn sol_to_lamports_guards_bad_input() {
        assert_eq!(sol_to_lamports(-1.0), 0);
        assert_eq!(sol_to_lamports(f64::NAN), 0);
        assert_eq!(sol_to_lamports(f64::INFINITY), 0);
    }

    #[test]
    fn parse_payload_real_sample() {
        let now = Instant::now();
        let snap = parse_payload(SAMPLE, now).unwrap();
        assert_eq!(snap.p25_lamports, 1_221); // 1.221e-6 SOL
        assert_eq!(snap.p50_lamports, 2_405); // 2.405e-6 SOL
        assert_eq!(snap.p75_lamports, 5_000); // 5e-6 SOL
        assert_eq!(snap.p99_lamports, 93_695); // 9.369504e-5 SOL
        assert_eq!(snap.taken_at, now);
    }

    // --- trend math ---

    fn snap_at(taken_at: Instant, p50: u64, p75: u64) -> TipSnapshot {
        TipSnapshot {
            taken_at,
            p25_lamports: 0,
            p50_lamports: p50,
            p75_lamports: p75,
            p95_lamports: 0,
            p99_lamports: 0,
            ema_p50_lamports: 0,
        }
    }

    #[test]
    fn trend_rising_over_window() {
        let now = Instant::now();
        let mut hist = VecDeque::new();
        hist.push_back(snap_at(now - Duration::from_secs(50), 1_000, 2_000));
        hist.push_back(snap_at(now - Duration::from_secs(25), 1_500, 2_500));
        hist.push_back(snap_at(now - Duration::from_secs(1), 1_800, 2_200));
        let trend = compute_trend(&hist, now, Duration::from_secs(60)).unwrap();
        assert_eq!(trend.p50_change_lamports, 800); // 1800 - 1000
        assert_eq!(trend.p75_change_lamports, 200); // 2200 - 2000
        assert!(trend.rising);
    }

    #[test]
    fn trend_falling_and_window_excludes_old() {
        let now = Instant::now();
        let mut hist = VecDeque::new();
        // This one is OUTSIDE a 30s window and must be excluded.
        hist.push_back(snap_at(now - Duration::from_secs(90), 9_999, 9_999));
        hist.push_back(snap_at(now - Duration::from_secs(20), 2_000, 3_000));
        hist.push_back(snap_at(now - Duration::from_secs(1), 1_200, 2_400));
        let trend = compute_trend(&hist, now, Duration::from_secs(30)).unwrap();
        assert_eq!(trend.p50_change_lamports, -800); // 1200 - 2000, not 9999
        assert!(!trend.rising);
    }

    #[test]
    fn trend_none_with_single_point_in_window() {
        let now = Instant::now();
        let mut hist = VecDeque::new();
        hist.push_back(snap_at(now - Duration::from_secs(100), 1_000, 1_000));
        hist.push_back(snap_at(now - Duration::from_secs(1), 1_500, 1_500));
        // Only the latest falls inside a 5s window.
        assert!(compute_trend(&hist, now, Duration::from_secs(5)).is_none());
    }

    #[test]
    fn trend_none_when_empty() {
        let hist = VecDeque::new();
        assert!(compute_trend(&hist, Instant::now(), Duration::from_secs(60)).is_none());
    }

    // --- mocked transport (offline) ---

    struct MockTransport {
        ws_frames: Vec<String>,
        rest_body: String,
    }

    impl TipTransport for MockTransport {
        async fn run_websocket(
            &self,
            sink: tokio::sync::mpsc::Sender<String>,
        ) -> anyhow::Result<()> {
            for frame in &self.ws_frames {
                if sink.send(frame.clone()).await.is_err() {
                    break;
                }
            }
            // Clean close so the supervisor exercises the REST-fallback path too.
            Ok(())
        }

        async fn fetch_rest(&self) -> anyhow::Result<String> {
            Ok(self.rest_body.clone())
        }
    }

    #[tokio::test]
    async fn supervisor_ingests_ws_then_rest_via_mock() {
        let transport = MockTransport {
            ws_frames: vec![SAMPLE.to_string()],
            rest_body: SAMPLE.to_string(),
        };
        let tracker = TipTracker::new(TipConfig::default(), transport);

        // run() loops forever; let it ingest the WS frame (and start REST
        // fallback) within a short window, then stop.
        let driver = tracker.clone();
        let _ = tokio::time::timeout(Duration::from_millis(150), async move {
            driver.run().await
        })
        .await;

        let latest = tracker.latest().expect("a snapshot was ingested");
        assert_eq!(latest.p50_lamports, 2_405);
        assert!(tracker.latest_source().is_some());
        assert!(!tracker.is_stale());
    }

    #[tokio::test]
    async fn rest_fallback_path_via_mock() {
        let transport = MockTransport {
            ws_frames: vec![],
            rest_body: SAMPLE.to_string(),
        };
        let tracker = TipTracker::new(TipConfig::default(), transport);
        // One REST poll (zero delay -> single iteration) exercises the trait.
        tracker.rest_poll_during(Duration::ZERO).await;
        assert_eq!(tracker.latest_source(), Some(TipSource::Rest));
        assert_eq!(tracker.latest().unwrap().p99_lamports, 93_695);
    }

    // --- staleness ---

    #[tokio::test]
    async fn staleness_logic() {
        let transport = MockTransport {
            ws_frames: vec![],
            rest_body: SAMPLE.to_string(),
        };
        let config = TipConfig {
            staleness_threshold: Duration::from_secs(30),
            ..Default::default()
        };
        let tracker = TipTracker::new(config, transport);

        // No data -> stale.
        assert!(tracker.is_stale());

        // Fresh snapshot -> not stale.
        tracker.store(snap_at(Instant::now(), 1, 1), TipSource::Rest);
        assert!(!tracker.is_stale());

        // Old snapshot -> stale.
        let old = Instant::now() - Duration::from_secs(40);
        tracker.store(snap_at(old, 1, 1), TipSource::Rest);
        assert!(tracker.is_stale());
        assert!(tracker.freshness().unwrap() >= Duration::from_secs(40));
    }
}
