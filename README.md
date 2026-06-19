# Smart Transaction Stack

A Rust service that submits Solana transactions through Jito bundles with the
discipline a production system needs: it watches the leader schedule over a
streaming connection, prices its tip from live auction data, tracks every
submission through its full commitment lifecycle, classifies failures from real
evidence, and lets an AI agent own one operational decision — what to do when a
bundle does not land.

For the full design rationale, see [ARCHITECTURE.md](./ARCHITECTURE.md).
For the central operational finding, see [FINDINGS.md](./FINDINGS.md).

---

## What it does

On every run the system:

- Streams slot updates and transaction statuses from a Yellowstone gRPC endpoint
  (the slot clock, and the signal it uses to see its own bundles land).
- Computes the next slot belonging to a Jito-enabled leader and submits one to
  two slots *ahead* of it, so the bundle is buffered at the block engine before
  that leader produces.
- Prices the tip from the live Jito tip-floor stream — never a hardcoded value.
- Builds a bundle, submits it through an authenticated, rate-limited path, and
  records it.
- Tracks each submission through Submitted → Processed → Confirmed → Finalized,
  stamping the slot and timestamp at each transition and computing the latency
  deltas.
- On a terminal failure, classifies *why* from the evidence and lets an AI agent
  decide whether to retry, reprice, refresh the blockhash, or abort.

---

## Workspace layout

Eight library crates plus a binary:

| Crate | Responsibility |
|-------|----------------|
| `stream` | Yellowstone gRPC ingestion (slots + tx status), reconnect supervisor, asymmetric backpressure |
| `leader` | Leader schedule ∩ Jito validator set; submit-ahead window logic; BAM detection |
| `tips` | Live tip-floor percentiles from Jito's websocket (REST fallback) |
| `submitter` | Bundle construction, signing, and submission; hot-path caches; the single authenticated, rate-limited block-engine choke point |
| `lifecycle` | SQLite commitment state machine; stream-driven confirmation; timeout sweeper |
| `failure` | Pure evidence-based failure classifier |
| `agent` | The AI operational decision (retry / reprice / refresh / abort), with a shadow baseline and guardrails |
| `orchestrator` | The control loop binary that composes all of the above |
| `runtime` | Shared init (crypto provider) and credential redaction helpers |

---

## Setup

### Prerequisites

- Rust (stable) and Cargo
- `protoc` (Protocol Buffers compiler) — required to build the Yellowstone client
  - macOS: `brew install protobuf`
- A funded mainnet wallet keypair
- Access credentials for:
  - A Yellowstone gRPC endpoint (slot + transaction-status streaming)
  - A Solana RPC endpoint (blockhash, signature status, simulation)
  - An Anthropic API key (for the failure-reasoning agent)
  - A Jito JSON-RPC UUID (for authenticated bundle submission — see note below)

### Configuration

Copy `.env.example` to `.env` and fill in the values:

```
# Streaming + RPC
YELLOWSTONE_ENDPOINT=https://<your-grpc-endpoint>:443
YELLOWSTONE_X_TOKEN=<token>
RPC_URL=https://<your-rpc-endpoint>

# Wallet
WALLET_PATH=./keys/smart-tx-wallet.json

# Jito block engine
JITO_BLOCK_ENGINE_URL=https://mainnet.block-engine.jito.wtf
JITO_AUTH_UUID=<your-jito-uuid>        # authenticated submission
JITO_RPS=2                             # rate limit your UUID permits

# Tip policy
TIP_PERCENTILE=p75                     # p50 | p75 | p95
MAX_TIP_LAMPORTS=100000

# Agent
ANTHROPIC_API_KEY=<key>

# BAM pricing (optional, off by default)
BAM_PRIORITY_FEE_ENABLED=false
BAM_PRIORITY_FEE_MICROLAMPORTS=0
BAM_PRIORITY_FEE_CU_LIMIT=10000
```

> **Authentication matters.** Jito's block engine accepts anonymous submissions,
> but in current conditions anonymous bundles rarely win their auction. A
> JSON-RPC UUID (requested through Jito's support flow and whitelisted to your
> submitting IP) authenticates every block-engine call and is effectively
> required to be competitive. See ARCHITECTURE.md §5.

### Build

```bash
cargo build --workspace
cargo test --workspace
```

### Run

```bash
# Submit N bundles and drain each to a terminal state
cargo run -p orchestrator -- run --count 10

# Inspect the current state of tracked bundles
cargo run -p orchestrator -- status

# Export the lifecycle log
cargo run -p orchestrator -- export

# Deliberately produce a classified failure (for testing the failure/agent path)
cargo run -p orchestrator -- fault sub-floor-tip
cargo run -p orchestrator -- fault stale-blockhash
```

---

## The three questions

These are the questions the system is built to answer correctly, with answers
grounded in what it actually measured on mainnet.

### Q1. What does the gap between Processed and Confirmed actually mean?

**Processed** means the transaction has been included in a block by the current
leader, but that block has not yet been voted on by the rest of the cluster — it
is one validator's claim, and it can still be dropped if that block does not
become part of the canonical chain (for example, if the leader's fork loses).
**Confirmed** means a supermajority of stake has voted on the block containing
the transaction; at that point it is extremely unlikely to be rolled back,
because reversing it would require the cluster to abandon a block a supermajority
already endorsed.

So the Processed → Confirmed delta is the time it takes for the rest of the
cluster to see and vote on the block your transaction landed in. With Solana
slots at ~400ms, that delta is typically one to two slots — a few hundred
milliseconds up to ~800ms — for the block to propagate and reach a supermajority
vote. This stack instruments that delta per bundle (the `process_to_confirm_ms`
column, stamped from the transaction-status stream), but it is honest about its
own data: in the runs to date no contentless bundle won its Jito auction to land
and exercise the Processed/Confirmed path, so that column is captured *by design*
yet *unmeasured here*. The directly measured latency in this system is on the
submission side — the hot-path send was driven from ~870ms to ~200ms (see
[ARCHITECTURE.md](./ARCHITECTURE.md) §5); the commitment-transition deltas await
a landing (see [FINDINGS.md](./FINDINGS.md)).

The practical meaning: **Processed is a landing, not a guarantee.** For anything
irreversible you wait for Confirmed (or Finalized); Processed tells you the
transaction made it into a candidate block, Confirmed tells you the cluster has
agreed to keep it.

### Q2. Why should you never fetch a blockhash at the `finalized` commitment for a time-sensitive transaction?

A transaction's `recent_blockhash` must refer to a block that is still inside the
cluster's ~150-slot blockhash window when the transaction is processed; once the
referenced block is more than ~150 slots behind the current slot, the blockhash
has expired and the transaction is rejected.

The three commitment levels return blockhashes of different ages. A `processed`
blockhash is the newest; a `confirmed` blockhash is a little older; a `finalized`
blockhash is the oldest, because finalization trails the chain tip by ~31–32
slots — a fixed consequence of Solana's vote-lockout depth, not a tunable. (This
stack relies on exactly that gap in its stale-blockhash fault, which fetches at
`finalized` to manufacture an aged blockhash for the classifier.) Fetching at `finalized`
therefore starts you tens of slots into the ~150-slot validity window before you
have even built the transaction — you are spending a large fraction of your
expiry budget on staleness you chose.

For a time-sensitive submission — especially a Jito bundle that may be retried —
that wasted budget is exactly what you cannot afford: the blockhash can expire
mid-flight or before a retry lands. So time-sensitive transactions fetch at
`confirmed` (a good balance of freshness and safety), and never at `finalized`.
This system caches a `confirmed` blockhash in the background and reads it from
memory at submission time, keeping the blockhash fresh without an RPC call in the
hot path. (The one exception is the deliberate stale-blockhash fault, which
fetches `finalized` precisely to manufacture an expired-blockhash failure for
testing the classifier.)

### Q3. What happens if the Jito leader skips its slot?

A leader can fail to produce a block in its assigned slot (it is offline, late,
or its block loses the fork). When that happens to the leader you targeted, your
bundle does not land in that slot — there is no block from that leader to land
in. The bundle was buffered at the block engine for a leader that never
produced.

The system handles this without treating it as a hard error. Because confirmation
is driven by the transaction-status stream rather than an assumption that the
target slot succeeded, a skipped slot simply means the memo signature never
appears. The lifecycle's timeout sweeper moves the bundle to a terminal
never-landed state after a bounded window, and the failure classifier reasons
about the cause: a never-landed bundle with a competitive tip and a leader that
did not produce points toward a skipped slot / lost auction rather than a
construction fault. The agent then decides the response — typically refresh the
blockhash and retry against the *next* Jito leader window, since the original
target is gone. The submit-ahead design also helps here: because the system
targets the start of an upcoming leader group rather than a single slot, a single
skipped slot within a healthy leader's group is less likely to be fatal than if
it had pinned everything on one exact slot.

---

## Failure handling and the AI decision

The system is built around failure, not the happy path. It produces and
classifies real failures:

- **TransportError** — network and rate-limit failures (including the block
  engine's HTTP 429 "globally rate limited" response), checked first so they are
  never misread as a pricing or construction fault.
- **FeeTooLow** — the tip was below what the auction required (the block engine
  returns an explicit minimum-tip message in some cases).
- **ExpiredBlockhash** — the referenced blockhash was already aged out of the
  ~150-slot window *at submission* (the genuine case, e.g. the stale-blockhash
  fault) — not merely because time passed while a bundle sat unlanded.
- **AuctionLost** — the block engine accepted the bundle (returned a `bundle_id`)
  but it never won its auction, confirmed by `getInflightBundleStatuses` returning
  `Invalid` (a `Certain` verdict) or inferred from a never-landed bundle with a
  valid-at-submission blockhash and a competitive tip (`Ambiguous`). This is the
  corrected classification for the central finding: a bundle that aged out *while
  waiting* lost the auction; the expiry is a downstream symptom, not the cause —
  so it is **not** reported as ExpiredBlockhash.

For each classified failure, the AI agent decides the response (retry, raise the
tip, refresh the blockhash, or abort), bounded by `max_attempts`. Every decision
runs alongside a deterministic baseline agent in a shadow A/B and is persisted,
so the model's choices are auditable against a simple rule. The agent's actions
pass through a guardrail clamp, and if the model call fails the system falls back
to the deterministic baseline rather than stalling.

---

## Lifecycle log

Each run records every submission and its full lifecycle to SQLite. Export the
log with:

```bash
cargo run -p orchestrator -- export
```

The exported log includes, per bundle: the bundle id, the memo signature, the tip
paid (with the p50/p75 market context at submit), the target slot, the
last Jito `getInflightBundleStatuses` verdict, the commitment transitions with
their slots and timestamps, the Processed → Confirmed → Finalized latency deltas
(populated when a bundle lands), and — for failed bundles — the classified failure
kind, the confidence, the evidence, and the agent's decision and rationale. (The
target *leader identity* and BAM flag are used at submit time but not persisted to
the row, so they are not in the export.)

A rendered, human-readable export of a real 58-bundle mainnet run is checked in as
[LIFECYCLE_LOG.md](./LIFECYCLE_LOG.md), with `exports/lifecycle_log.json` as the
machine-readable companion. Every bundle in it is `AuctionLost` — the finding in
[FINDINGS.md](./FINDINGS.md), shown in the data.

---

## A note on landing

Reaching a landed bundle on mainnet is a competitive-auction outcome, not purely
a function of correct code. This system was driven to the point where every
controllable variable is provably correct — construction, funding, tip account,
tip competitiveness, simulation, timing, authentication, and region — and the
remaining factor is the auction itself. ARCHITECTURE.md §5 documents that
investigation in full, including the resolution from Jito support that anonymous
bundles rarely win and that the two Jito auctions are scored on tip-and-fee
efficiency. The system's value is that it is instrumented to tell the truth about
its own behaviour at every stage, so that outcome is something it measures rather
than assumes.
