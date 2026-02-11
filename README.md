# Subspace Chain Pulse

Blockchain monitoring, alerting, and cross-domain message (XDM) transfer indexing for Subspace networks.

## Quick Start

### Alerter

- Save the Slack token to a file named `slack-secret`
- Restrict permissions (Unix): `chmod 400 slack-secret`
- `docker run --mount type=bind,source=/path/to/slack-secret,target=/slack-secret,readonly ghcr.io/autonomys/chain-alerter --rpc-url wss://rpc.mainnet.autonomys.xyz/ws --slack-bot-name "My Bot" --slack-channel-name chain-alerts`

### Indexer

Requires a PostgreSQL instance:

```bash
cargo run -p indexer -- --db-uri postgres://user:pass@localhost:5432/indexer
```

## What it does

### Alerter

Connects to a Subspace node via WebSocket and monitors for:
- **Block events**: known account transfers (deposits, withdrawals), domain upgrades, fraud proofs, operator slashing/offline, sudo calls, runtime code updates
- **Chain stalls and reorgs**: detects when blocks stop being produced or when forks exceed a depth threshold
- **Slot timing**: monitors per-slot and average slot duration via Proof-of-Time from the P2P network
- **Uptime**: optional Uptime Kuma health check pushes

Alerts are posted to a Slack channel. The network (Mainnet, Chronos Testnet, etc.) is auto-detected from node metadata, and the corresponding accounts and bootnodes are loaded from `alerter/networks.toml`.

### Indexer

REST API for querying cross-domain message (XDM) transfers, backed by PostgreSQL:
- `GET /health` — last processed block per chain
- `GET /v1/xdm/transfers/{address}` — XDM transfers for an address
- `GET /v1/xdm/recent` — recent XDM transfers (configurable limit)

Processes blocks in parallel from both the consensus chain and Auto-EVM domain.

## How block tracking and reorg detection works

The `shared/subspace.rs` module (`Subspace::listen_for_all_blocks`) manages block tracking and reorg detection. It subscribes to all imported blocks and maintains a header metadata cache (`HeadersMetadataCache`) of the last 100 blocks.

### Block processing flow

1. **Subscribe** to all blocks via `subscribe_all()`. On startup, load the last `CACHE_HEADER_DEPTH` (100) canonical block headers into the cache.
2. For each received block:
   a. Add its header to the cache.
   b. Check if it is the **canonical block** at that height by querying the node RPC (`is_canonical_block`). Fork blocks are logged and skipped.
   c. Compute the **tree route** from the previous best block to the new best block using `sp_blockchain::tree_route`, which walks the cached headers to find enacted (new best path) and retracted (old best path) blocks relative to a common ancestor.
   d. If headers are missing from the cache during tree route computation, fetch them from RPC and retry (`recursive_tree_route`).
3. **Broadcast** the enacted blocks as `BlocksExt` via a `tokio::broadcast` channel. If blocks were retracted, include `ReorgData` (enacted blocks, retracted blocks, common ancestor).
4. After broadcasting, **prune** cached headers older than 100 blocks behind the current best.

### How consumers handle reorgs

The `stall_and_reorg` alerter listens on the broadcast stream and:
- If `BlocksExt` contains `ReorgData` with retracted blocks exceeding the configured `--reorg-depth-threshold` (default 6), it sends a reorg alert to Slack.
- If no blocks arrive within `--non-block-import-threshold` (default 60s), it sends a chain stall alert. When blocks resume, it sends a recovery alert.

### RPC reconnection

If the block subscription closes due to an RPC error, the listener automatically re-subscribes and reloads the header cache from the new connection.

For details of the fork choice algorithm, see [the subspace protocol specification](https://subspace.github.io/protocol-specs/docs/decex/workflow#fork-choice-rule).

## Security notes

- The Slack OAuth token is loaded from a file and must be readable only by the current user.
  - On Unix, the process enforces `0400` or `0600` permissions. Other modes cause a panic at startup.
- The token is wrapped and zeroized on drop to reduce in-memory exposure.
- Do not commit or log the token. The `.gitignore` and `Debug` impl handle this by default.

### Managing the Slack Bot

The Slack bot has permission to read and post in channels it is invited into by Slack users.

The Slack bot can be managed via your Slack login on [the Slack apps portal](https://api.slack.com/apps):

- The Autonomys Slack team ID is T03LJ85UR5G and the App ID is A09956363PY (these are not secrets)
- Slack bot tokens can be created and revoked on the [Install App](https://api.slack.com/apps/A09956363PY/install-on-team) screen, and the bot's permissions can be changed
- Bot tokens look like `xoxb-(numbers)-(numbers)-(base64)`

## Getting started

### Prerequisites

- Rust nightly (pinned in `rust-toolchain.toml`)
- A Slack bot token with permission to post to the target channel in the Autonomys workspace (for alerter)
- PostgreSQL (for indexer)
- A running Subspace node (a local non-archival node is fine for the alerter)

### Prepare Slack secret file (alerter)

1. Get the current Slack "bot token" from [Install App on the Slack apps portal](https://api.slack.com/apps/A09956363PY/install-on-team)
2. Save it to a file named `slack-secret`
3. Restrict permissions (Unix): `chmod 400 slack-secret` (or `chmod 600 slack-secret`)

### Build and run the alerter

```bash
cargo run -p alerter -- \
  --rpc-url wss://rpc.mainnet.autonomys.xyz/ws \
  --slack-bot-name "My Bot" \
  --slack-channel-name chain-alerts \
  --slack-secret-path ./slack-secret
```

#### Alerter CLI arguments

| Argument | Required | Default | Description |
|---|---|---|---|
| `--rpc-url` | Yes | — | Node WebSocket RPC endpoint |
| `--network-config-path` | No | `/networks.toml` | Path to TOML file with accounts and bootnodes |
| `--slack-bot-name` | Yes | — | Bot display name in Slack |
| `--slack-channel-name` | Yes | — | Target Slack channel |
| `--slack-secret-path` | No | `/slack-secret` | Path to file containing bot token |
| `--slack-bot-icon` | No | `robot_face` | Bot emoji icon |
| `--slack-team-id` | No | `T03LJ85UR5G` | Slack workspace ID |
| `--uptimekuma-url` | No | — | Uptime Kuma push URL |
| `--uptimekuma-interval` | No | `60s` | Health check push frequency |
| `--non-block-import-threshold` | No | `60s` | Alert after no blocks for this duration |
| `--reorg-depth-threshold` | No | `6` | Reorg depth to trigger alert |
| `--per-slot-threshold` | No | `1.2s` | Max acceptable per-slot duration |
| `--avg-slot-threshold` | No | `1.1s` | Max acceptable average slot duration |

### Build and run the indexer

```bash
cargo run -p indexer -- \
  --db-uri postgres://indexer:password@localhost:5432/indexer?sslmode=disable
```

#### Indexer CLI arguments

| Argument | Required | Default | Description |
|---|---|---|---|
| `--migrations-path` | No | `./indexer/migrations` | Path to SQL migration files |
| `--consensus-rpc` | No | `wss://rpc.mainnet.autonomys.xyz/ws` | Consensus chain RPC |
| `--auto-evm-rpc` | No | `wss://auto-evm.mainnet.autonomys.xyz/ws` | Auto-EVM domain RPC |
| `--db-uri` | No | `postgres://indexer:password@localhost:5434/indexer?sslmode=disable` | PostgreSQL connection string |
| `--process-blocks-in-parallel` | No | `5000` | Number of blocks to process concurrently |

All indexer arguments can also be set via environment variables (uppercase, e.g. `DB_URI`, `CONSENSUS_RPC`).

### Logging

`RUST_LOG` can be used to filter logs. See [EnvFilter docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives).

### Running a local Subspace node (optional)

Running a local node gives low latency for block subscriptions, best block checks, and event retrieval. See the [Subspace monorepo](https://github.com/autonomys/subspace) for build/run instructions.

## Project structure

- **`alerter/`** — Chain event alerting service
  - `main.rs`: multi-task orchestrator using `tokio::JoinSet`
  - `cli.rs`: command-line configuration (clap)
  - `events.rs`: block event monitoring (transfers, domain events, fraud proofs, operator events, sudo, code updates)
  - `stall_and_reorg.rs`: chain stall detection and reorg monitoring
  - `slots.rs`: slot timing monitoring via Proof-of-Time
  - `p2p_network.rs`: libp2p peer discovery and PoT stream collection
  - `slack.rs`: Slack API integration with secure token handling
  - `uptime.rs`: Uptime Kuma health check pusher
  - `event_types.rs`: alert event type definitions
  - `md_format.rs`: markdown formatting for alert messages
  - `networks.toml`: per-network configuration (known accounts, bootstrap nodes)
- **`indexer/`** — XDM transfer indexer and REST API
  - `main.rs`: Actix-web server + dual-chain block processors
  - `api.rs`: REST endpoints (health, XDM transfers)
  - `xdm.rs`: XDM event processing and indexing
  - `storage.rs`: PostgreSQL database layer (sqlx)
  - `types.rs`: domain models (ChainId, transfer types)
  - `migrations/`: SQL migration files
- **`shared/`** — Shared blockchain client utilities
  - `subspace.rs`: subxt-based chain interaction (block streaming, metadata, events)
- **`docker/`** — Multi-platform Dockerfiles (`alerter.Dockerfile`, `indexer.Dockerfile`)

### References

- Subspace Protocol reference implementation (node): [autonomys/subspace](https://github.com/autonomys/subspace)
