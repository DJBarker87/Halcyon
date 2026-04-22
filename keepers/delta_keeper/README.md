# Delta Keeper

Flagship `AggregateDelta` publisher.

It reads live flagship notes plus live Pyth SPY/QQQ/IWM prices, computes per-note analytical deltas through the host-only flagship pricing path, builds a Merkle artifact, pins that artifact to Pinata, then submits the paired `[ed25519, write_aggregate_delta]` transaction to the kernel.

## Runtime shape

- Product program ID is compiled in from `halcyon_flagship_autocall`: `E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc`
- Kernel program ID is compiled in from `halcyon_kernel`: `H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF`
- The keeper does not take runtime program IDs in JSON config
- The signer must be the pubkey currently registered in `KeeperRegistry.delta`
- This is not the Observation keeper path unless `KeeperRegistry.delta` and `KeeperRegistry.observation` intentionally resolve to the same pubkey on the target cluster

## Pinata

The keeper uses Pinata's current REST pinning API directly; there is no Rust SDK dependency in this crate.

- Create an account at `pinata.cloud`.
- Create an API key / JWT in the Pinata dashboard.
- Export that JWT only in the environment:

```bash
export PINATA_JWT=REPLACE_ME
```

- Base URL: `https://api.pinata.cloud`
- Endpoint: `POST /pinning/pinJSONToIPFS`
- Auth: `Authorization: Bearer $PINATA_JWT`
- Body fields used by the keeper: `pinataContent`, `pinataMetadata`, `pinataOptions`

`PINATA_JWT` is required at runtime and must stay in the environment, not in `config/delta_keeper.json`.

## Config Creation

Start from the tracked template:

```bash
cp config/examples/delta_keeper.example.json config/delta_keeper.json
```

Then set the devnet values the current keeper expects:

- `rpc_endpoint`: Helius devnet URL, for example `https://devnet.helius-rpc.com/?api-key=REPLACE_ME`
- `keypair_path`: local path to the key registered as `KeeperRegistry.delta`
- `pyth_spy`: `Dix5qyQ52TtpErut1z6DUqmzVbqNfyCcjf1XvArJnwwY`
- `pyth_qqq`: `4Np4uL4vYhumfDAZeootLniVqrGhaK83Vh9WcQJwANmG`
- `pyth_iwm`: `GoUfqZ5jdJEi4gnLUoajt5PLqHyVzbUAi8KRGaBczYsZ`
- `merkle_output_path`: writable local path for the JSON artifact

The tracked [config/delta_keeper.json](/Users/dominic/colosseumfinal/config/delta_keeper.json:1) already carries those current devnet values plus informational underscore-prefixed fields for the compiled kernel / flagship program IDs and signer requirement.

## Keypair Requirements

- `write_aggregate_delta` authenticates against `KeeperRegistry.delta`, not `KeeperRegistry.observation`
- the local devnet key in this repo is `ops/devnet_keys/delta.json`
- if your target cluster intentionally reuses the Observation key for Delta, the pubkeys must already match on-chain before you point this keeper at that file

## Local One-Shot Run

1. Make sure `ops/devnet_keys/delta.json` exists and its pubkey matches `KeeperRegistry.delta`.
2. Export `PINATA_JWT`.
3. Run:

```bash
cargo run --release -p delta_keeper -- --config config/delta_keeper.json --once
```

Expected outputs:

- local artifact written to `/tmp/halcyon_flagship_delta.json`
- Pinata CID returned in logs
- `write_aggregate_delta` submitted successfully

## Hetzner Deploy

For deployed devnet infra, use `ops/hetzner-deploy/deploy.sh`. It renders `/etc/halcyon/config/delta_keeper.json` from `/etc/halcyon/env`, including:

- `HELIUS_DEVNET_RPC`
- `DELTA_KEYPAIR`
- `PYTH_SPY_ACCOUNT`
- `PYTH_QQQ_ACCOUNT`
- `PYTH_IWM_ACCOUNT`
- `PINATA_BASE_URL`
- `PINATA_RETRIES`

The Pinata JWT stays only in `/etc/halcyon/env` and is read by the systemd service at runtime.

Deployment shape:

- systemd unit: `ops/hetzner-deploy/systemd/halcyon-delta-keeper.service`
- process model: continuous daemon, not a timer
- keeper cadence: 30 seconds by default via `DELTA_SCAN_INTERVAL_SECS`
- enable flag: `ENABLE_DELTA_KEEPER=1`
- secret env: `PINATA_JWT`
