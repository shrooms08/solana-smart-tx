//! Yellowstone gRPC (Dragon's Mouth) ingestion.
//!
//! Subscribes to slot updates and transaction-status updates and republishes
//! them to the rest of the stack as a normalized [`StreamEvent`] stream.
//!
//! The public entry point is [`StreamClient::run`]: a long-running supervisor
//! that connects, subscribes, decodes updates, and forwards them onto a bounded
//! `tokio::sync::mpsc` channel — reconnecting with exponential backoff whenever
//! the connection drops. The receive loop does *minimal* work (decode + forward
//! only); all DB / business logic lives downstream of the channel.
//!
//! Backpressure is asymmetric (see [`StreamClient::run`]):
//!   * slot updates are latest-wins, so they're dropped (and counted) when the
//!     channel is full;
//!   * transaction-status updates must never be lost, so they block until the
//!     consumer drains room.

use std::time::{Duration, Instant, SystemTime};

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcClient};
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, SubscribeRequest, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions, SubscribeUpdate, SubscribeUpdateSlot,
    SubscribeUpdateTransactionStatus,
};

// ---------------------------------------------------------------------------
// Tuning constants
// ---------------------------------------------------------------------------

/// First reconnect delay; doubles each failed attempt up to [`BACKOFF_CAP`].
const BACKOFF_BASE: Duration = Duration::from_millis(250);
/// Upper bound on the reconnect delay.
const BACKOFF_CAP: Duration = Duration::from_secs(30);
/// Growth factor between attempts.
const BACKOFF_FACTOR: f64 = 2.0;
/// A connection that stayed up at least this long is considered "healthy"; the
/// backoff schedule is reset to [`BACKOFF_BASE`] once it drops, so a long-lived
/// link that blips doesn't inherit a punishing delay.
const HEALTHY_RESET: Duration = Duration::from_secs(60);
/// How often to emit the cumulative dropped-slot counter while a connection is
/// live (only logged when the count actually moved).
const DROPPED_REPORT_INTERVAL: Duration = Duration::from_secs(10);
/// Capacity of the request-side channel feeding the gRPC subscription. Holds a
/// single `SubscribeRequest`; the rest is slack.
const REQUEST_CHANNEL_CAPACITY: usize = 8;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Connection parameters for the Yellowstone endpoint.
///
/// These mirror the `YELLOWSTONE_ENDPOINT` / `YELLOWSTONE_X_TOKEN` fields of the
/// orchestrator's `Config`; the caller is responsible for plumbing them in.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// gRPC endpoint URL (e.g. `https://example.rpcpool.com:10000`).
    pub endpoint: String,
    /// `x-token` auth header, if the provider requires one.
    pub x_token: Option<String>,
}

// ---------------------------------------------------------------------------
// Normalized domain types
// ---------------------------------------------------------------------------

/// Commitment / lifecycle status carried on a slot update.
///
/// Mirrors the proto `SlotStatus` enum. The first three are the standard
/// commitment levels; the rest are interslot signals the server can emit but
/// that we don't subscribe to by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotStatus {
    Processed,
    Confirmed,
    Finalized,
    FirstShredReceived,
    Completed,
    CreatedBank,
    Dead,
}

impl SlotStatus {
    /// Map the proto enum's `i32` wire value onto our status.
    ///
    /// Unknown values (a forward-compatible server adding a variant) fall back
    /// to [`SlotStatus::Processed`] rather than panicking.
    fn from_proto(value: i32) -> Self {
        use yellowstone_grpc_proto::prelude::SlotStatus as Proto;
        match Proto::try_from(value) {
            Ok(Proto::SlotProcessed) => Self::Processed,
            Ok(Proto::SlotConfirmed) => Self::Confirmed,
            Ok(Proto::SlotFinalized) => Self::Finalized,
            Ok(Proto::SlotFirstShredReceived) => Self::FirstShredReceived,
            Ok(Proto::SlotCompleted) => Self::Completed,
            Ok(Proto::SlotCreatedBank) => Self::CreatedBank,
            Ok(Proto::SlotDead) => Self::Dead,
            Err(_) => Self::Processed,
        }
    }
}

/// A normalized slot update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotUpdate {
    pub slot: u64,
    pub status: SlotStatus,
    /// Wall-clock time the update was decoded locally (ingest timestamp).
    pub received_at: SystemTime,
}

/// A normalized transaction-status update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxStatusUpdate {
    /// Base58 transaction signature.
    pub signature: String,
    pub slot: u64,
    /// `None` on success; otherwise an opaque, hex-encoded representation of the
    /// proto `TransactionError` payload. Decoding the bincode body into a typed
    /// error is deliberately left to downstream consumers — the receive loop
    /// only decodes and forwards.
    pub err: Option<String>,
    /// Wall-clock time the update was decoded locally (ingest timestamp).
    pub received_at: SystemTime,
}

/// Events emitted by the ingestion stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    Slot(SlotUpdate),
    TxStatus(TxStatusUpdate),
}

// ---------------------------------------------------------------------------
// Reconnect backoff
// ---------------------------------------------------------------------------

/// Exponential backoff schedule with full jitter.
///
/// The *ceiling* for a given attempt is `min(cap, base * factor^attempt)`; the
/// delay actually returned by [`Backoff::next_delay`] is a uniform random draw
/// in `(0, ceiling]` ("full jitter"), which avoids thundering-herd reconnects.
///
/// The un-jittered [`Backoff::ceiling`] is exercised by unit tests; the jittered
/// path is only validated against a live endpoint via the `slot_probe` example.
#[derive(Debug, Clone)]
struct Backoff {
    base: Duration,
    cap: Duration,
    factor: f64,
    attempt: u32,
}

impl Backoff {
    fn new(base: Duration, cap: Duration, factor: f64) -> Self {
        Self {
            base,
            cap,
            factor,
            attempt: 0,
        }
    }

    /// Reset back to the first interval after a healthy connection.
    fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Deterministic, un-jittered delay ceiling for the current attempt.
    fn ceiling(&self) -> Duration {
        let scaled = self.base.as_secs_f64() * self.factor.powi(self.attempt as i32);
        let capped = scaled.min(self.cap.as_secs_f64());
        Duration::from_secs_f64(capped)
    }

    /// Return the (jittered) delay for the current attempt and advance.
    fn next_delay(&mut self) -> Duration {
        let ceiling = self.ceiling();
        self.attempt = self.attempt.saturating_add(1);
        // Full jitter: uniform in (0, ceiling].
        let factor = rand::random::<f64>();
        Duration::from_secs_f64(ceiling.as_secs_f64() * factor)
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(BACKOFF_BASE, BACKOFF_CAP, BACKOFF_FACTOR)
    }
}

// ---------------------------------------------------------------------------
// Decode helpers (pure, unit-tested)
// ---------------------------------------------------------------------------

/// Map a proto slot update onto our normalized [`SlotUpdate`].
fn decode_slot(update: SubscribeUpdateSlot, received_at: SystemTime) -> SlotUpdate {
    SlotUpdate {
        slot: update.slot,
        status: SlotStatus::from_proto(update.status),
        received_at,
    }
}

/// Map a proto transaction-status update onto our normalized [`TxStatusUpdate`].
fn decode_tx_status(
    update: SubscribeUpdateTransactionStatus,
    received_at: SystemTime,
) -> TxStatusUpdate {
    let signature = Signature::try_from(update.signature.as_slice())
        .map(|s| s.to_string())
        // Fall back to a hex dump if the bytes aren't a valid 64-byte signature;
        // we still want to forward *something* rather than drop the update.
        .unwrap_or_else(|_| hex_encode(&update.signature));

    let err = update.err.map(|e| hex_encode(&e.err));

    TxStatusUpdate {
        signature,
        slot: update.slot,
        err,
        received_at,
    }
}

/// Lowercase hex with a `0x` prefix; used for the opaque tx-error payload (and
/// the cold-path malformed-signature fallback).
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("0x");
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

// ---------------------------------------------------------------------------
// Subscription request
// ---------------------------------------------------------------------------

/// Build the `SubscribeRequest` for our two filters:
///   * `slots`: all slot status updates (processed / confirmed / finalized).
///   * `transactions`: every (including failed) tx touching a monitored pubkey.
fn build_subscribe_request(monitored: &[Pubkey]) -> SubscribeRequest {
    let mut slots = std::collections::HashMap::new();
    slots.insert(
        "slots".to_owned(),
        SubscribeRequestFilterSlots {
            // `false` => don't filter by the request commitment: deliver every
            // status (processed, confirmed, finalized).
            filter_by_commitment: Some(false),
            // We only want the three commitment statuses, not the interslot
            // (first-shred / completed / created-bank) firehose.
            interslot_updates: Some(false),
        },
    );

    let mut transactions = std::collections::HashMap::new();
    transactions.insert(
        "monitored".to_owned(),
        SubscribeRequestFilterTransactions {
            // Include failures — a failed tx from our wallet is exactly what the
            // failure-analysis path needs to see.
            failed: Some(true),
            // Don't constrain on vote-ness; the account filter keeps vote txs
            // out anyway since our wallet never votes.
            vote: None,
            signature: None,
            account_include: monitored.iter().map(|p| p.to_string()).collect(),
            account_exclude: Vec::new(),
            account_required: Vec::new(),
        },
    );

    SubscribeRequest {
        slots,
        transactions,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Entry point for Yellowstone ingestion. Stateless — all methods are
/// associated functions; construct nothing, just call [`StreamClient::run`].
#[derive(Debug)]
pub struct StreamClient;

/// How a single connection's lifecycle ended.
enum ConnEnd {
    /// The server closed the stream cleanly; reconnect.
    ServerClosed,
    /// Transport / status error; reconnect.
    Disconnected(anyhow::Error),
    /// The downstream event consumer was dropped; stop the supervisor.
    ConsumerGone,
}

/// Outcome of one connect+subscribe+receive cycle.
struct ConnOutcome {
    /// How long the subscription stayed live (used to decide a backoff reset).
    uptime: Duration,
    end: ConnEnd,
}

/// Result of forwarding a single update.
enum ForwardResult {
    Continue,
    ConsumerGone,
}

impl StreamClient {
    /// Connect to the Yellowstone endpoint with `x-token` auth and native TLS.
    ///
    /// This is the cleanest low-level helper: it performs the TCP/TLS/gRPC
    /// handshake and returns a ready [`GeyserGrpcClient`]. Subscription is
    /// driven separately so the supervisor owns the reconnect lifecycle.
    pub async fn connect(config: &StreamConfig) -> anyhow::Result<GeyserGrpcClient> {
        let client = GeyserGrpcClient::build_from_shared(config.endpoint.clone())?
            .x_token(config.x_token.clone())?
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .connect()
            .await?;
        Ok(client)
    }

    /// Long-running ingestion entry point.
    ///
    /// Connects, subscribes to slots + transactions touching `monitored_pubkeys`,
    /// decodes every update into a [`StreamEvent`], and forwards it on `event_tx`.
    /// Runs forever, reconnecting with exponential backoff + jitter, until the
    /// `event_tx` consumer is dropped (clean shutdown) — then returns `Ok(())`.
    ///
    /// Backpressure policy is asymmetric:
    ///   * **Slot updates** use `try_send`; on a full channel the event is
    ///     dropped and a cumulative `dropped_slots` counter is incremented and
    ///     logged periodically. Slot updates are latest-wins, so dropping is safe.
    ///   * **Transaction-status updates** use `send().await`, guaranteeing
    ///     delivery even under load — they must never be dropped.
    pub async fn run(
        config: StreamConfig,
        monitored_pubkeys: Vec<Pubkey>,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<()> {
        let request = build_subscribe_request(&monitored_pubkeys);
        let mut backoff = Backoff::default();
        let mut dropped_slots: u64 = 0;
        let mut attempt: u64 = 0;

        loop {
            attempt += 1;
            info!(
                attempt,
                endpoint = %config.endpoint,
                monitored = monitored_pubkeys.len(),
                "connecting to Yellowstone"
            );

            let outcome =
                Self::run_connection(&config, &request, &event_tx, &mut dropped_slots).await;

            // A connection that lived long enough is "healthy": reset the
            // schedule so a single blip doesn't inherit a long delay.
            if outcome.uptime >= HEALTHY_RESET {
                backoff.reset();
            }

            match outcome.end {
                ConnEnd::ConsumerGone => {
                    info!("event consumer dropped; shutting down Yellowstone stream");
                    return Ok(());
                }
                ConnEnd::ServerClosed => {
                    warn!(uptime = ?outcome.uptime, "Yellowstone stream closed by server");
                }
                ConnEnd::Disconnected(err) => {
                    warn!(uptime = ?outcome.uptime, error = %err, "Yellowstone stream disconnected");
                }
            }

            let delay = backoff.next_delay();
            warn!(?delay, next_attempt = attempt + 1, "backing off before reconnect");
            tokio::time::sleep(delay).await;
        }
    }

    /// Run exactly one connect+subscribe+receive cycle, returning when the
    /// connection ends for any reason.
    async fn run_connection(
        config: &StreamConfig,
        request: &SubscribeRequest,
        event_tx: &mpsc::Sender<StreamEvent>,
        dropped_slots: &mut u64,
    ) -> ConnOutcome {
        // Time the connect attempt too; a connect that fails instantly yields a
        // ~zero uptime, so the backoff won't reset.
        let started = Instant::now();

        let mut client = match Self::connect(config).await {
            Ok(client) => client,
            Err(err) => {
                return ConnOutcome {
                    uptime: started.elapsed(),
                    end: ConnEnd::Disconnected(err.context("connect failed")),
                };
            }
        };

        // The request side of the bidi stream: a channel the gRPC client polls.
        // We send our single SubscribeRequest, then keep the sender alive for
        // the life of the connection so the server doesn't see a half-close.
        let (mut req_tx, req_rx) = futures::channel::mpsc::channel(REQUEST_CHANNEL_CAPACITY);
        if let Err(err) = req_tx.send(request.clone()).await {
            return ConnOutcome {
                uptime: started.elapsed(),
                end: ConnEnd::Disconnected(anyhow::anyhow!(
                    "failed to send subscribe request: {err}"
                )),
            };
        }

        let mut stream = match client.geyser.subscribe(req_rx).await {
            Ok(response) => response.into_inner(),
            Err(status) => {
                return ConnOutcome {
                    uptime: started.elapsed(),
                    end: ConnEnd::Disconnected(
                        anyhow::Error::new(status).context("subscribe failed"),
                    ),
                };
            }
        };

        // Connection is established; measure uptime from here.
        let connected_at = Instant::now();
        info!(endpoint = %config.endpoint, "Yellowstone connected; streaming");
        let _req_tx = req_tx; // keep the request stream open for this connection

        let mut report = tokio::time::interval(DROPPED_REPORT_INTERVAL);
        report.tick().await; // consume the immediate first tick
        let mut last_reported = *dropped_slots;

        loop {
            tokio::select! {
                message = stream.next() => match message {
                    Some(Ok(update)) => {
                        match Self::forward(update, event_tx, dropped_slots).await {
                            ForwardResult::Continue => {}
                            ForwardResult::ConsumerGone => {
                                return ConnOutcome {
                                    uptime: connected_at.elapsed(),
                                    end: ConnEnd::ConsumerGone,
                                };
                            }
                        }
                    }
                    Some(Err(status)) => {
                        return ConnOutcome {
                            uptime: connected_at.elapsed(),
                            end: ConnEnd::Disconnected(anyhow::Error::new(status)),
                        };
                    }
                    None => {
                        return ConnOutcome {
                            uptime: connected_at.elapsed(),
                            end: ConnEnd::ServerClosed,
                        };
                    }
                },
                _ = report.tick() => {
                    if *dropped_slots != last_reported {
                        warn!(
                            dropped_slots = *dropped_slots,
                            "dropping slot updates: event channel full (latest-wins)"
                        );
                        last_reported = *dropped_slots;
                    }
                }
            }
        }
    }

    /// Decode one proto update and forward it onto the event channel, applying
    /// the asymmetric backpressure policy. Non-slot/tx updates (pings, etc.) are
    /// ignored. Returns whether the consumer is still alive.
    async fn forward(
        update: SubscribeUpdate,
        event_tx: &mpsc::Sender<StreamEvent>,
        dropped_slots: &mut u64,
    ) -> ForwardResult {
        let now = SystemTime::now();
        match update.update_oneof {
            Some(UpdateOneof::Slot(slot)) => {
                let event = StreamEvent::Slot(decode_slot(slot, now));
                // Latest-wins: drop on a full channel, but stop on a closed one.
                match event_tx.try_send(event) {
                    Ok(()) => ForwardResult::Continue,
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        *dropped_slots += 1;
                        ForwardResult::Continue
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => ForwardResult::ConsumerGone,
                }
            }
            Some(UpdateOneof::TransactionStatus(tx)) => {
                let event = StreamEvent::TxStatus(decode_tx_status(tx, now));
                // Guaranteed delivery: block until there's room.
                match event_tx.send(event).await {
                    Ok(()) => ForwardResult::Continue,
                    Err(_) => ForwardResult::ConsumerGone,
                }
            }
            other => {
                // Pings/pongs and any filters we didn't ask for: ignore.
                if let Some(other) = other {
                    debug!(variant = ?std::mem::discriminant(&other), "ignoring non-slot/tx update");
                }
                ForwardResult::Continue
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// These cover the pure logic that doesn't need a network: the backoff schedule
// and the decode mapping. The *real* end-to-end verification is the
// `slot_probe` example run against a live Yellowstone endpoint.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_ceiling_grows_then_caps() {
        let base = Duration::from_millis(250);
        let cap = Duration::from_secs(30);
        let mut b = Backoff::new(base, cap, 2.0);

        // attempt 0 -> base, 1 -> 2x, 2 -> 4x, ... until capped.
        assert_eq!(b.ceiling(), Duration::from_millis(250));
        b.attempt = 1;
        assert_eq!(b.ceiling(), Duration::from_millis(500));
        b.attempt = 2;
        assert_eq!(b.ceiling(), Duration::from_secs(1));
        b.attempt = 7;
        // 250ms * 2^7 = 32s, clamped to the 30s cap.
        assert_eq!(b.ceiling(), Duration::from_secs(30));
        b.attempt = 30;
        assert_eq!(b.ceiling(), cap);
    }

    #[test]
    fn backoff_reset_returns_to_base() {
        let mut b = Backoff::default();
        for _ in 0..5 {
            let _ = b.next_delay();
        }
        assert!(b.attempt > 0);
        b.reset();
        assert_eq!(b.attempt, 0);
        assert_eq!(b.ceiling(), BACKOFF_BASE);
    }

    #[test]
    fn backoff_jittered_delay_within_ceiling() {
        let mut b = Backoff::default();
        for _ in 0..100 {
            let ceiling = b.ceiling();
            let delay = b.next_delay();
            assert!(delay <= ceiling, "delay {delay:?} exceeded ceiling {ceiling:?}");
        }
    }

    #[test]
    fn slot_status_maps_all_proto_variants() {
        use yellowstone_grpc_proto::prelude::SlotStatus as Proto;
        assert_eq!(SlotStatus::from_proto(Proto::SlotProcessed as i32), SlotStatus::Processed);
        assert_eq!(SlotStatus::from_proto(Proto::SlotConfirmed as i32), SlotStatus::Confirmed);
        assert_eq!(SlotStatus::from_proto(Proto::SlotFinalized as i32), SlotStatus::Finalized);
        assert_eq!(
            SlotStatus::from_proto(Proto::SlotFirstShredReceived as i32),
            SlotStatus::FirstShredReceived
        );
        assert_eq!(SlotStatus::from_proto(Proto::SlotDead as i32), SlotStatus::Dead);
        // Unknown wire value falls back rather than panicking.
        assert_eq!(SlotStatus::from_proto(9999), SlotStatus::Processed);
    }

    #[test]
    fn decode_slot_maps_fields() {
        let now = SystemTime::now();
        let proto = SubscribeUpdateSlot {
            slot: 42,
            parent: Some(41),
            status: yellowstone_grpc_proto::prelude::SlotStatus::SlotConfirmed as i32,
            dead_error: None,
        };
        let decoded = decode_slot(proto, now);
        assert_eq!(decoded.slot, 42);
        assert_eq!(decoded.status, SlotStatus::Confirmed);
        assert_eq!(decoded.received_at, now);
    }

    #[test]
    fn decode_tx_status_success_and_failure() {
        let now = SystemTime::now();
        let sig_bytes = vec![7u8; 64]; // valid 64-byte signature length

        let ok = SubscribeUpdateTransactionStatus {
            slot: 100,
            signature: sig_bytes.clone(),
            is_vote: false,
            index: 0,
            err: None,
        };
        let decoded = decode_tx_status(ok, now);
        assert_eq!(decoded.slot, 100);
        assert!(decoded.err.is_none());
        // 64 bytes of 0x07 base58-encodes to a stable, non-empty signature.
        assert!(!decoded.signature.is_empty());

        let failed = SubscribeUpdateTransactionStatus {
            slot: 101,
            signature: sig_bytes,
            is_vote: false,
            index: 1,
            err: Some(yellowstone_grpc_proto::prelude::TransactionError {
                err: vec![0xde, 0xad],
            }),
        };
        let decoded = decode_tx_status(failed, now);
        assert_eq!(decoded.err.as_deref(), Some("0xdead"));
    }

    #[test]
    fn subscribe_request_has_both_filters() {
        let pk = Pubkey::new_unique();
        let req = build_subscribe_request(&[pk]);
        assert_eq!(req.slots.len(), 1);
        assert_eq!(req.transactions.len(), 1);
        let tx_filter = req.transactions.values().next().unwrap();
        assert_eq!(tx_filter.failed, Some(true));
        assert_eq!(tx_filter.account_include, vec![pk.to_string()]);
    }
}
