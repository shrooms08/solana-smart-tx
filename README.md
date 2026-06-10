# Smart Transaction Stack

Solana transaction infrastructure — Yellowstone gRPC streaming, Jito bundle submission with dynamic tips, full lifecycle tracking, and AI-driven failure reasoning. Built for the Superteam Nigeria Advanced Infrastructure Challenge.

## Status

 **Work in progress.** Foundation crates are implemented and verified against live infrastructure; the submission, tracking, and reasoning layers are next.

| Crate          | Role                                                             | Status |
| -------------- | ---------------------------------------------------------------- | :----: |
| `stream`       | Yellowstone gRPC (Dragon's Mouth) slot + transaction streaming   |   ✅   |
| `leader`       | Jito leader-window detection (slot clock × leader schedule)      |   ✅   |
| `tips`         | Jito bundle-tip market-data feed (WebSocket + REST fallback)     |   ✅   |
| `submitter`    | Jito bundle construction & submission                            |   🚧   |
| `lifecycle`    | Per-bundle lifecycle tracking & persistence                      |   🚧   |
| `failure`      | Failure classification                                           |   🚧   |
| `agent`        | AI-driven failure reasoning                                      |   🚧   |
| `orchestrator` | Wiring + control loop across all crates                          |   🚧   |

## Architecture

An eight-crate Cargo workspace, each crate a single responsibility behind a small public API, composed by the `orchestrator`. Data flows in one direction: **stream → leader → submit → track → classify → agent**. The `stream` crate ingests live slots and transaction statuses from Yellowstone; `leader` combines that slot clock with the leader schedule and the Jito validator set to know when a Jito leader is about to be up; `tips` keeps current tip percentiles fresh; `submitter` builds and fires bundles into those windows with appropriate tips; `lifecycle` tracks each bundle to a terminal state; `failure` classifies what went wrong; and `agent` reasons over those failures to inform future attempts.

## Setup

### Prerequisites

- **Rust** (stable) — install via [rustup](https://rustup.rs).
- **protoc** (Protocol Buffers compiler) — **required**: the build fails without it because the Yellowstone gRPC proto types are generated at compile time.
  - macOS: `brew install protobuf`
  - Debian/Ubuntu: `apt install protobuf-compiler`

### Configure

```sh
cp .env.example .env
# then fill in .env
```

Environment variables (see `.env.example`):

| Variable                | Description                                            |
| ----------------------- | ------------------------------------------------------ |
| `RPC_URL`               | Solana JSON-RPC endpoint                               |
| `YELLOWSTONE_ENDPOINT`  | Yellowstone gRPC (Dragon's Mouth) endpoint             |
| `YELLOWSTONE_X_TOKEN`   | Optional `x-token` auth for the Yellowstone provider   |
| `JITO_BLOCK_ENGINE_URL` | Jito block-engine region base URL                      |
| `WALLET_KEYPAIR_PATH`   | Path to the wallet keypair JSON (kept out of git)      |
| `ANTHROPIC_API_KEY`     | API key for the reasoning agent                        |
| `RUST_LOG`              | Optional tracing verbosity (e.g. `info`)               |

Secrets live only in your local `.env` / environment — never commit them. `.env` and `*.json` keypairs are gitignored.

### Build

```sh
cargo build
```

Each implemented crate ships a runnable probe example against live infrastructure, e.g.:

```sh
cargo run --example slot_probe -p stream     # live Yellowstone slot stream
cargo run --example leader_probe -p leader   # live Jito leader windows
cargo run --example tip_probe -p tips        # live Jito tip feed
```

## Documentation

The full architecture document and operational writeup are coming as separate deliverables.
