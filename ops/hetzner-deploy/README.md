# Hetzner Halcyon Keeper Deployment Kit

This kit brings up the current Halcyon devnet keeper backend on a fresh
Ubuntu 22.04 Hetzner box with root SSH access.

The current repo splits into two operational styles:

- long-running daemons in `keepers/`
- one-shot CLI jobs in `tools/halcyon_cli`, scheduled by systemd timers

The deploy defaults below enable the current demo-critical path:

- `halcyon-price-relay`
- `halcyon-observation-keeper`
- `halcyon-regime-keeper`
- IL / SOL EWMA timers
- flagship sigma timer
- SOL reduced-operator timer
- flagship autocall-schedule timer

The heavier / less stable pieces stay off by default:

- delta keeper
- SOL hedge keeper
- flagship hedge keeper
- legacy regression daemon
- recurring regression timer

## Delta Keeper Prereqs

Before enabling `ENABLE_DELTA_KEEPER=1`, confirm all of the following:

- `DELTA_KEYPAIR` points at the key whose pubkey is registered on-chain as `KeeperRegistry.delta`
- `PINATA_JWT` is populated in `/etc/halcyon/env`; the keeper will abort its publish cycle without it
- `PYTH_SPY_ACCOUNT`, `PYTH_QQQ_ACCOUNT`, and `PYTH_IWM_ACCOUNT` are the current deterministic devnet relay accounts
- the binary you deploy is built from the checkout whose compiled program IDs match the current devnet kernel and flagship programs

The current devnet IDs used by this repo are:

- Kernel: `H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF`
- Flagship: `E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc`

## Before You Start

1. Make sure the server can clone the Halcyon repo.
   If `HALCYON_REPO_URL` is an SSH GitHub URL, install a GitHub deploy key on the server first.
2. Prepare `/etc/halcyon/env` from [env.example](/Users/dominic/colosseumfinal/ops/hetzner-deploy/env.example:1).
3. Prepare keeper keypairs under `/root/halcyon-keys/` per [keys/README.md](/Users/dominic/colosseumfinal/ops/hetzner-deploy/keys/README.md:1).
4. Upload the flagship regression calibration CSVs from the old repo:
   - `../Colosseum/halcyon-hedge-lab/data/cache/spy_1d.csv`
   - `../Colosseum/halcyon-hedge-lab/data/cache/qqq_1d.csv`
   - `../Colosseum/halcyon-hedge-lab/data/cache/iwm_1d.csv`

## Local Commands

Replace `root@YOUR_SERVER_IP` everywhere.

Create the directories first:

```bash
ssh root@YOUR_SERVER_IP '
  mkdir -p /etc/halcyon /etc/halcyon/config /etc/halcyon/calibration /root/halcyon-keys &&
  chmod 700 /root/halcyon-keys
'
```

Upload the kit:

```bash
scp -r /Users/dominic/colosseumfinal/ops/hetzner-deploy root@YOUR_SERVER_IP:/root/
```

Upload your edited env file:

```bash
scp /path/to/halcyon.env root@YOUR_SERVER_IP:/etc/halcyon/env
```

Upload keeper keys:

```bash
scp /Users/dominic/colosseumfinal/ops/devnet_keys/observation.json root@YOUR_SERVER_IP:/root/halcyon-keys/observation.json
scp /Users/dominic/colosseumfinal/ops/devnet_keys/regression.json root@YOUR_SERVER_IP:/root/halcyon-keys/regression.json
scp /Users/dominic/colosseumfinal/ops/devnet_keys/delta.json root@YOUR_SERVER_IP:/root/halcyon-keys/delta.json
scp /Users/dominic/colosseumfinal/ops/devnet_keys/hedge.json root@YOUR_SERVER_IP:/root/halcyon-keys/hedge.json
scp /Users/dominic/colosseumfinal/ops/devnet_keys/regime.json root@YOUR_SERVER_IP:/root/halcyon-keys/regime.json
scp /path/to/price-relay-keypair.json root@YOUR_SERVER_IP:/root/halcyon-keys/price-relay.json
scp /path/to/il-ewma.json root@YOUR_SERVER_IP:/root/halcyon-keys/il-ewma.json
scp /path/to/sol-ewma.json root@YOUR_SERVER_IP:/root/halcyon-keys/sol-ewma.json
scp /path/to/flagship-ewma.json root@YOUR_SERVER_IP:/root/halcyon-keys/flagship-ewma.json
scp /path/to/flagship-sigma.json root@YOUR_SERVER_IP:/root/halcyon-keys/flagship-sigma.json
```

Upload regression calibration CSVs:

```bash
scp /Users/dominic/Colosseum/halcyon-hedge-lab/data/cache/spy_1d.csv root@YOUR_SERVER_IP:/etc/halcyon/calibration/spy_1d.csv
scp /Users/dominic/Colosseum/halcyon-hedge-lab/data/cache/qqq_1d.csv root@YOUR_SERVER_IP:/etc/halcyon/calibration/qqq_1d.csv
scp /Users/dominic/Colosseum/halcyon-hedge-lab/data/cache/iwm_1d.csv root@YOUR_SERVER_IP:/etc/halcyon/calibration/iwm_1d.csv
```

Lock down permissions once uploads are done:

```bash
ssh root@YOUR_SERVER_IP '
  chmod 600 /etc/halcyon/env /root/halcyon-keys/*.json
'
```

Run the deployment:

```bash
ssh root@YOUR_SERVER_IP 'bash /root/hetzner-deploy/deploy.sh'
```

Wait 90-120 seconds for the timers to fire at least once, then verify:

```bash
ssh root@YOUR_SERVER_IP 'bash /root/hetzner-deploy/verify.sh'
```

## What deploy.sh Does

- installs Rust, Node.js 20, `tsx`, Python 3, Solana CLI 2.3.0, git, jq, and native build deps
- clones Halcyon into `/opt/halcyon`
- checks out `HALCYON_REF` if set, otherwise `HALCYON_BRANCH`
- builds native keeper binaries and `halcyon` CLI in release mode
- installs systemd units from `ops/hetzner-deploy/systemd`
- renders keeper JSON config files into `/etc/halcyon/config/`
- enables the units and timers selected in `/etc/halcyon/env`

## Important Operational Notes

- The repo does not currently contain a `rust-toolchain.toml`. The deploy script therefore uses `RUST_TOOLCHAIN_VERSION` from `/etc/halcyon/env`, default `1.93.0`.
- Flagship pricing and regression still need all of `SPY_USD`, `QQQ_USD`, and `IWM_USD`. The flagship sigma keeper itself only consumes `SPY` history from Pyth Benchmarks.
- Pyth Benchmarks currently works without an API key. The sigma keeper fetches daily bars in <=1-year chunks because the history endpoint rejects wider windows.
- The relay maintains a best-effort local cache at `/var/lib/halcyon/relay-cache/`, one CSV per feed, pruned weekly according to `PRICE_RELAY_CACHE_RETENTION_DAYS`.
- `delta_keeper` pins its Merkle artifact through Pinata's `pinJSONToIPFS` API using `PINATA_JWT` from `/etc/halcyon/env`. The generated JSON config does not carry this secret.
- `delta_keeper` does not read runtime kernel / flagship program IDs from JSON. Those IDs are compiled into the binary via the linked crates, so a mismatched checkout and deployed programs will fail semantically even if the config file looks correct.
- `flagship-sigma` reuses the Observation keeper authority on-chain. The file can be a copy of `observation.json`, but it must resolve to the same pubkey.
- `write-regression` is disabled by default as a recurring timer because the current workable path uses static CSV uploads. Re-running it without refreshing the CSVs will fail the on-chain monotonicity check once the same `window_end_ts` repeats.
- `flagship_hedge_keeper` is scaffold/dry-run only in this repo. Leave it masked unless you explicitly want the dry-run logs.
- `fire-reduced-ops` reuses the Regime keeper authority. If that key is not the registered `KeeperRegistry.regime`, the timer will fail.
- `write-autocall-schedule` and `flagship-sigma` both reuse the Observation keeper authority. If that key is not the registered `KeeperRegistry.observation`, those timers will fail.
- Building the Rust workspace on a 2 vCPU / 2 GB VM can take a while. Expect roughly 10-20 minutes on a cold host.
- `CARGO_BUILD_JOBS=1` is set in the example env on purpose. Higher parallelism is likely to OOM a CPX11 during `cargo build --release`.
