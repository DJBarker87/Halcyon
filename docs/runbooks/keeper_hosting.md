# Keeper Hosting Runbook

Getting the five (six, once flagship hedge lands live) Halcyon keepers
running on a VPS as supervised systemd services. Assumes Ubuntu 22.04 LTS;
the shape is identical on Debian 12 or any systemd-based distro. Ballpark
cost is `$75–100/mo` on mainnet, mostly RPC (see
`docs/runbooks/mainnet_runbook.md` for the launch sequence this slots
into).

---

## 0. Pre-reqs

- A fresh VPS, ≥2 GB RAM, 2 vCPU, static IP. Hetzner CPX21 or
  DigitalOcean 4 GB droplet is fine.
- A paid Solana RPC endpoint (Helius / Triton / QuickNode). Public RPC
  rate-limits will silently lose transactions on mainnet — do not skip
  this.
- An IPFS pinning service JWT. Pinata's free tier (1 GB / 100 pins)
  overflows in under an hour at the delta keeper's 30s cadence; budget
  for the $20/mo paid tier.
- A Jupiter API key (free).
- A funded keeper keypair per role: observation, regression, delta,
  hedge, regime, flagship-hedge. Each keypair must be registered in
  `KeeperRegistry` before its keeper will succeed on-chain.
- Enough SOL on each keeper keypair to cover transaction fees —
  5 SOL/month is a safe starting buffer; the delta keeper dominates.

---

## 1. Host provisioning

```
# on the VPS, as root
adduser --disabled-password --gecos "" halcyon
mkdir -p /etc/halcyon/keys /etc/halcyon/env /var/lib/halcyon /var/log/halcyon
chown -R halcyon:halcyon /etc/halcyon /var/lib/halcyon /var/log/halcyon
chmod 700 /etc/halcyon/keys /etc/halcyon/env
apt-get update && apt-get install -y build-essential pkg-config libssl-dev curl ufw
ufw allow OpenSSH && ufw --force enable
```

Directory layout once populated:

```
/opt/halcyon/bin/           # compiled keeper binaries
/etc/halcyon/config/        # <keeper>.json configs (committed-safe, no secrets)
/etc/halcyon/env/           # <keeper>.env files (mode 0600, SECRETS)
/etc/halcyon/keys/          # <keeper>.keypair.json (mode 0600, SECRETS)
/var/lib/halcyon/           # keeper state: merkle artefact mirror etc.
/var/log/halcyon/           # (if shipping to disk instead of journald)
```

---

## 2. Build pattern

Two options; pick one.

**Build on host (simplest):**

```
apt-get install -y rustup
sudo -u halcyon bash -c 'curl -sSf https://sh.rustup.rs | sh -s -- -y'
sudo -u halcyon bash -lc '
  git clone https://github.com/<you>/colosseumfinal /home/halcyon/src
  cd /home/halcyon/src
  cargo build --release -p delta_keeper -p hedge_keeper -p flagship_hedge_keeper \
              -p regression_keeper -p regime_keeper -p observation_keeper
  install -m 0755 target/release/{delta_keeper,hedge_keeper,flagship_hedge_keeper,regression_keeper,regime_keeper,observation_keeper} \
    /opt/halcyon/bin/
'
```

**Cross-compile on a build box and scp:** builds are small (~15 MB
stripped per binary). Use if you don't want a toolchain on the host.

```
# on build box
cargo build --release --target x86_64-unknown-linux-gnu -p delta_keeper ...
scp target/x86_64-unknown-linux-gnu/release/delta_keeper halcyon@vps:/opt/halcyon/bin/
```

---

## 3. Keypair and config placement

For each keeper, from a secure offline machine:

```
# generate on an air-gapped or hardware-backed host
solana-keygen new --outfile /tmp/delta_keeper.keypair.json
# register the pubkey in KeeperRegistry via the admin multisig — see mainnet_runbook.md
# then transfer the keypair file to the VPS over a confidential channel:
scp /tmp/delta_keeper.keypair.json halcyon@vps:/etc/halcyon/keys/
ssh halcyon@vps 'chmod 600 /etc/halcyon/keys/delta_keeper.keypair.json'
# on an offline machine: shred the temp copy
shred -u /tmp/delta_keeper.keypair.json
```

Fund each keeper's pubkey. 1 SOL is more than enough as a starting
buffer; top up from the monitoring dashboard.

Place each keeper's config at `/etc/halcyon/config/<keeper>.json`
starting from `config/examples/<keeper>.example.json`. Set
`keypair_path` to `/etc/halcyon/keys/<keeper>.keypair.json`.

Secrets go in `/etc/halcyon/env/<keeper>.env` (never in the JSON
config — the config is fine to version-control):

```
# /etc/halcyon/env/delta_keeper.env
PINATA_JWT=eyJhbGci…
RUST_LOG=info
```

```
# /etc/halcyon/env/hedge_keeper.env
JUPITER_API_KEY=…
RUST_LOG=info
```

```
# /etc/halcyon/env/flagship_hedge_keeper.env
JUPITER_API_KEY=…
RUST_LOG=info
```

`chmod 600` every env file. `chown halcyon:halcyon` everything.

---

## 4. systemd units

Templates live in `ops/systemd/`. Copy each `.service` to
`/etc/systemd/system/`, then enable and start. They all run as the
`halcyon` user, read their env file, and restart on exit.

```
cp ops/systemd/halcyon-*.service /etc/systemd/system/
cp ops/systemd/halcyon-keepers.target /etc/systemd/system/

systemctl daemon-reload
systemctl enable --now halcyon-delta-keeper halcyon-hedge-keeper \
  halcyon-regression-keeper halcyon-regime-keeper halcyon-observation-keeper
# flagship-hedge-keeper stays masked until the live-submit path ships
systemctl mask halcyon-flagship-hedge-keeper

# start/stop everything at once:
systemctl start halcyon-keepers.target
systemctl status halcyon-keepers.target
```

Each unit writes JSON logs to the journal. Tail with:

```
journalctl -u halcyon-delta-keeper -f --output=cat | jq .
```

---

## 5. Monitoring and alerts

The keepers already emit structured JSON logs with a heartbeat per
cycle. Wire them to at least one of:

**journald → Grafana Loki (cheapest):** install promtail on the VPS,
ship `/var/log/journal` entries to Grafana Cloud free tier (50 GB/mo).
Build a dashboard on `target = "halcyon_*"` labels.

**Prometheus scrape:** the SOL Autocall hedge keeper already exposes
Prometheus counters (see `ops/monitoring/prometheus/`); extend the same
pattern to the flagship keeper when its live-submit path lands.

Pages to configure (minimum viable):

- any keeper systemd unit enters `failed` state
- any keeper's last heartbeat is > 5× its `scan_interval_secs`
- keeper keypair SOL balance < 0.1 SOL (tx fees exhausted)
- RPC round-trip p95 > 2 s sustained for 5 min
- Pinata pin failure on delta keeper (alert after 3 consecutive failures)

A scrappy first-cut health check is `systemctl list-units --failed`
piped to a cron that hits a PagerDuty / Slack webhook.

---

## 6. SOL balance top-ups

Fund the keeper pubkeys from a treasury wallet when balance falls below
the alert threshold. For each:

```
# from your treasury machine
solana transfer --from ~/.config/solana/id.json \
  <delta_keeper_pubkey> 1 --allow-unfunded-recipient
```

Keep a simple cron on the VPS that checks balances and prints to the
journal; the alert fires off the journal entry.

---

## 7. Failover RPC

Each keeper config takes a single `rpc_endpoint`. For a two-endpoint
failover:

- Run a tiny nginx/HAProxy on the VPS on `localhost:8899` that
  upstream-proxies to `helius` primary + `triton` secondary with
  health checks.
- Point all keeper configs at `http://127.0.0.1:8899`.
- Trivially swappable; also lets you change RPC providers without
  touching the keepers.

If you don't want the proxy complexity, set up a second VPS in a
different region pointing at a different RPC, and use DNS failover.
That's a separate, heavier decision — fine to defer until you've felt
RPC flakiness in production.

---

## 8. Updating / rollback

```
# on the build box (or host if you build there)
git pull
cargo build --release -p delta_keeper
scp target/release/delta_keeper halcyon@vps:/opt/halcyon/bin/delta_keeper.new
# on the VPS
systemctl stop halcyon-delta-keeper
install -m 0755 -o halcyon -g halcyon /opt/halcyon/bin/delta_keeper.new /opt/halcyon/bin/delta_keeper
systemctl start halcyon-delta-keeper
journalctl -u halcyon-delta-keeper -f --output=cat | jq .
```

Keep the previous binary around for one week in case of rollback:

```
mv /opt/halcyon/bin/delta_keeper /opt/halcyon/bin/delta_keeper.$(date +%F)
```

---

## 9. Flagship hedge keeper special note

`flagship_hedge_keeper` is currently scaffold-only — no live Jupiter
submission. Ship the unit file with `systemctl mask` set, or omit
entirely until the live-submit path lands. Running the scaffold against
mainnet is harmless (it dry-runs) but burns RPC quota and emits logs
that look like a real hedge would, which is confusing during incident
review.

When the live-submit path lands:

1. Unmask the unit: `systemctl unmask halcyon-flagship-hedge-keeper`.
2. Confirm `KeeperRegistry.hedge == flagship_hedge_keeper_pubkey` (or a
   split per-product authority once `KeeperRegistry` grows one).
3. Start in `--dry-run` on devnet first; validate one full rebalance
   cycle end-to-end before unmasking for mainnet.
4. Follow `docs/audit/OPEN_QUESTIONS.md` for the flagship unpause
   predicate.

---

## 10. Smoke test checklist (day 0)

- [ ] all unit files exist and `systemctl status` is `active (running)`
- [ ] each keeper wrote its first on-chain update within one scan
      interval (check `last_update_ts` on each account)
- [ ] Pinata pin for the delta keeper's first artifact resolves via
      `https://gateway.pinata.cloud/ipfs/<cid>`
- [ ] `research/tools/verify_aggregate_delta.py` exits 0 against the
      first on-chain AggregateDelta
- [ ] no journal entries at ERROR level across any keeper for 30 min
- [ ] every keeper keypair balance > 0.5 SOL
- [ ] monitoring dashboard shows a heartbeat per scan interval
