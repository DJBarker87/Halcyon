# Halcyon price relay

Pulls Pyth VAAs from Hermes and posts them to `PriceUpdateV2` accounts on
the target Solana cluster. Exists because Pyth publishes SPY/QQQ/IWM to
mainnet only — Halcyon's flagship pricer needs those feeds on whatever
cluster we're running against.

The other keepers in this repo are Rust. This one is TypeScript because
the Pyth-blessed SDK (`@pythnetwork/pyth-solana-receiver`) handles the
full Wormhole VAA verification + receiver `post_updates` dance in one
helper; reimplementing that in Rust is ~6 hours of Wormhole-program
orchestration that nobody needs to write.

## One-time install (VPS)

```
# on the VPS
sudo apt-get install -y nodejs npm
sudo -u halcyon bash -c '
  mkdir -p /opt/halcyon/price_relay
  cd /opt/halcyon/price_relay
  git clone --sparse --depth 1 https://github.com/<you>/colosseumfinal .
  git sparse-checkout set keepers/price_relay
  cd keepers/price_relay
  npm install
'
```

Or, if building locally and shipping:
```
cd keepers/price_relay
npm install
rsync -a ./ halcyon@vps:/opt/halcyon/price_relay/
```

## Config

Copy `config/devnet.example.json` to `/etc/halcyon/config/price_relay.json`
and edit:

| field | required | notes |
|---|---|---|
| `rpc_endpoint` | yes | Paid Helius endpoint recommended; public RPC rate-limits the Wormhole verifies |
| `hermes_endpoint` | yes | `https://hermes.pyth.network` — no auth required |
| `keypair_path` | yes | ~1 devnet SOL covers months of tx fees |
| `shard_id` | yes | Any u16. Determines the PDA for each feed's PriceUpdateV2. Keep stable. |
| `scan_interval_secs` | yes | 10s is a good default; 2s is the floor. |
| `staleness_cap_secs` | yes | Skip posts when the last on-chain publish_time is less than this old. Saves fees. |
| `cache_dir` | no | Defaults to `/var/lib/halcyon/relay-cache`. One CSV per feed lands here. |
| `cache_retention_days` | no | Defaults to `450`. Weekly prune keeps enough history for sigma + warmup. |
| `cache_feeds` | no | Defaults to all configured feed aliases. Lets you disable cache writes for specific feeds. |
| `feeds` | yes | `{ alias, id }` pairs. Feed IDs live in `crates/halcyon_oracles/src/lib.rs` as hex literals. |
| `failure_budget` | yes | Exit after this many consecutive cycles fail, so alerting fires. |

Set the Helius API key in `/etc/halcyon/env/price_relay.env` if you
prefer env var injection over hard-coding it in the JSON — the systemd
unit reads that env file optionally.

## Print the deterministic feed addresses

Before the first post lands, you need the 5 PDAs the relay will
write — so you can pin them in `frontend/.env.local` and in the other
keepers' configs. The relay computes them offline:

```
cd keepers/price_relay
npm run addresses
```

Output:

```
shard_id = 7
receiver = rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ

SOL_USD
  account : <pubkey>
  env     : NEXT_PUBLIC_PYTH_SOL_ACCOUNT_DEVNET=<pubkey>

USDC_USD
  account : <pubkey>
  env     : NEXT_PUBLIC_PYTH_USDC_ACCOUNT_DEVNET=<pubkey>

...
```

Paste each `env` line into `frontend/.env.local`, and paste the raw
pubkeys into `keepers/delta_keeper/...` + `flagship_hedge_keeper` +
`observation_keeper` configs as appropriate.

## Smoke test once

```
npm install
npm run start:devnet -- --once
```

Should print `posted` lines for any feeds whose on-chain publish_time
is stale vs. Hermes. If nothing is stale on the first run (unlikely for
a brand new shard), it prints `no on-chain updates needed this cycle`.

If cache is enabled, successful post cycles also append one row per feed to:

```text
{cache_dir}/{feed_alias}.csv
```

Schema:

```text
publish_time,price,conf,exponent
```

The writer dedupes on `publish_time` within each feed file and prunes rows
older than `cache_retention_days` on startup and then weekly.

## Run under systemd

```
# On the VPS
sudo cp /opt/halcyon/price_relay/keepers/price_relay/../../ops/systemd/halcyon-price-relay.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now halcyon-price-relay
journalctl -u halcyon-price-relay -f --output=cat | jq .
```

Logs are JSON lines to journald. Grep for `"level":"ERROR"` to find
cycle failures.

## Feed address migration

If you change `shard_id` in config, ALL feed PDAs change. You'll need
to re-run `npm run addresses`, update frontend + keepers, and the old
accounts go stale on-chain (rent remains until someone closes them).
Keep the shard stable once production is running.

## Operational notes

- **CU per post**: ~200K for the Wormhole verify + ~50K for the receiver
  write. Fits comfortably in one tx at a 50K µlamport priority fee.
- **Devnet Hermes rate limits**: none observed. Pyth explicitly markets
  Hermes as "free public endpoint; best-effort."
- **Finality wait**: we confirm at `confirmed`, not `finalized`. A
  finalized-only consumer would need to wait another ~13 seconds.
- **Staleness cap**: the relay reads the on-chain publish_time before
  posting; if it's within `staleness_cap_secs` of the Hermes
  publish_time, it skips. This saves fees during weekends / market
  close when equity feeds don't move.
- **Cache durability**: the relay treats cache writes as best-effort.
  A failed cache append never rolls back a successful on-chain post.
