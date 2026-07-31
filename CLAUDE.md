# CLAUDE.md

Rust workspace (nightly, edition 2024) for Autonomys/Subspace chain monitoring: `alerter`
posts Slack alerts, `indexer` serves XDM + staking REST endpoints backed by PostgreSQL,
`shared` wraps subxt.

## Gotchas

- Tests need PostgreSQL *installed* locally, not running: `pgtemp` shells out to `initdb`
  and `postgres` to spin up a throwaway instance per test.
- Match CI's test invocation exactly: `cargo -Zgitoxide -Zgit nextest run --locked`.
- The alerter refuses to start unless the Slack token file is mode 0400 or 0600, and
  zeroizes the token on drop. Keep both ends of that contract.
- Watched accounts and `bootnodes` are per-network in `alerter/networks.toml`, keyed by
  display name (`Autonomys Mainnet`, `Autonomys Chronos Testnet`).

## Releases

Tag-driven, not merge-driven. `release.yml` fires on `v*.*.*` and asserts the tag equals `v` +
`[workspace.package].version` from the root `Cargo.toml`, failing before it publishes anything
on a mismatch. So bump the version on `main` in its own commit, then tag that commit.

A matching tag publishes `ghcr.io/autonomys/` images for both services and opens a **draft**
release, left for a human to review and publish. Image bumps and container redeploys are
coordinated from `autonomys/infra`; nothing deploys from here.
