# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rust workspace for real-time Subspace blockchain monitoring, alerting, and cross-domain message (XDM) transfer indexing. Three crates: `alerter` (Slack alerts for chain events), `indexer` (REST API for XDM transfers with PostgreSQL), and `shared` (blockchain client utilities via subxt).

## Build & CI Commands

```bash
# Format check (CI enforces this)
cargo fmt --all -- --check

# Lint (CI runs with --release)
cargo clippy --release --locked --all-features --all-targets -- -D warnings

# Run tests (requires PostgreSQL installed locally — tests use pgtemp for isolated instances)
cargo -Zgitoxide -Zgit nextest run --locked

# Build
cargo build --release              # development release
cargo build --profile=production   # production (LTO enabled)
```

The toolchain is **nightly** (pinned in `rust-toolchain.toml` to `nightly-2025-05-31`). Edition is 2024.

## Workspace Lints

Configured in the root `Cargo.toml`:
- `warnings = "deny"` — all warnings are errors
- `unsafe_code = "forbid"` — no unsafe code allowed
- `rust_2018_idioms = "deny"`

## Code Formatting

`rustfmt.toml` uses `edition = "2024"` and `imports_granularity = "Module"` (group imports by module).

## Architecture

### alerter
Multi-task orchestrator using `tokio::JoinSet`. Each concern runs as an independent async task communicating via `tokio::broadcast` channels:
- **Block event watcher** (`events.rs`) — monitors balance transfers, domain upgrades, fraud proofs, operator/sudo/governance events
- **Stall & reorg detector** (`stall_and_reorg.rs`) — tree-based fork tracking, alerts on chain stalls and reorgs above configurable thresholds
- **Slot timing monitor** (`slots.rs`) — tracks per-slot and average slot duration
- **P2P network** (`p2p_network.rs`) — libp2p Kademlia peer discovery, Proof-of-Time stream collection
- **Slack handler** (`slack.rs`) — posts alerts with rate limiting (up to 30 retries); loads bot token from a file with enforced Unix permissions (0400/0600) and zeroizes on drop
- **Uptime pusher** (`uptime.rs`) — Uptime Kuma health checks

Network-specific configuration (known accounts, bootstrap nodes) lives in `alerter/networks.toml`.

### indexer
Actix-web HTTP server (port 8080) with parallel block processors for consensus and domain chains:
- **REST API** (`api.rs`):
  - `GET /health` — last processed block per chain
  - `GET /v1/xdm/transfers/{address}` — XDM transfers for an address
  - `GET /v1/xdm/recent` — recent transfers (configurable limit)
- **XDM processing** (`xdm.rs`) — indexes OutgoingTransferInitiated, Executed, Failed, IncomingTransferSuccessful events; checkpoints every 100 blocks; parallel processing via `futures::stream::buffered`
- **Storage** (`storage.rs`) — PostgreSQL via sqlx with upsert logic; migrations in `indexer/migrations/`

### shared
Blockchain interaction layer using subxt:
- `subspace.rs` — block streaming with fork detection, runtime metadata updates, storage queries, event reading
- Key types: `Block`, `BlockHash`, `BlockNumber`, `AccountId`, `Balance`, `Slot`, `Timestamp`, `BlockExt`, `SubspaceBlockProvider`

## Docker

Dockerfiles in `docker/` support multi-platform builds (amd64 v2/v3, arm64). Images published to `ghcr.io/autonomys/`.

## Key Patterns

- **Channel-based task communication**: `tokio::broadcast` for pub/sub decoupling between async tasks
- **Scale codec**: `parity-scale-codec` for blockchain data serialization/decoding
- **Error handling**: `thiserror` with per-crate error enums; no panics in normal flow
- **Secrets**: file-based token loading with strict Unix permissions + zeroize-on-drop
