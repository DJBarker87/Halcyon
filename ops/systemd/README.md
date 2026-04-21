# Halcyon keeper systemd units

Drop-in service files for running the Halcyon keeper fleet on a Linux
host under systemd. Full deployment walkthrough lives at
`docs/runbooks/keeper_hosting.md`.

## Files

| File | Role |
|---|---|
| `halcyon-delta-keeper.service` | Writes flagship `AggregateDelta` + Pinata pin |
| `halcyon-hedge-keeper.service` | SOL Autocall hedge executor (Jupiter) |
| `halcyon-flagship-hedge-keeper.service` | Flagship hedge — **mask until live-submit lands** |
| `halcyon-regression-keeper.service` | Writes flagship `Regression` (IWM→SPY/QQQ β) |
| `halcyon-regime-keeper.service` | Writes `RegimeSignal` |
| `halcyon-observation-keeper.service` | SOL Autocall observation writes |
| `halcyon-price-relay.service` | Pyth VAA → devnet PriceUpdateV2 accounts (Node.js, see `keepers/price_relay/`) |
| `halcyon-keepers.target` | Groups the production keepers |
| `halcyon-keeper.env.example` | Template env file (secrets go here) |

## Install

```
sudo cp halcyon-*.service /etc/systemd/system/
sudo cp halcyon-keepers.target /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now halcyon-keepers.target
sudo systemctl mask halcyon-flagship-hedge-keeper   # until live-submit ships
```

## Expected on-host layout

```
/opt/halcyon/bin/<keeper>               # compiled release binary
/etc/halcyon/config/<keeper>.json       # tracked config (no secrets)
/etc/halcyon/env/<keeper>.env           # 0600, secrets (PINATA_JWT, JUPITER_API_KEY, RUST_LOG)
/etc/halcyon/keys/<keeper>.keypair.json # 0600, the registered keeper authority
/var/lib/halcyon/                       # keeper-writable state
```

All units run as user `halcyon:halcyon`, restart on failure, and use
the systemd hardening surface (`NoNewPrivileges`, `ProtectSystem=strict`,
`MemoryDenyWriteExecute`, etc.).

## Logs

Structured JSON to the journal; tail with:

```
journalctl -u halcyon-delta-keeper -f --output=cat | jq .
```
