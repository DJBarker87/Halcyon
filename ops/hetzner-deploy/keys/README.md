# Key Placement

Put keeper keypairs on the server under `/root/halcyon-keys/`. Do not commit
them. Upload them separately with `scp`, then `chmod 600 /root/halcyon-keys/*.json`.

Recommended file names:

- `/root/halcyon-keys/price-relay.json`
- `/root/halcyon-keys/observation.json`
- `/root/halcyon-keys/regression.json`
- `/root/halcyon-keys/delta.json`
- `/root/halcyon-keys/hedge.json`
- `/root/halcyon-keys/regime.json`
- `/root/halcyon-keys/flagship-hedge.json`
- `/root/halcyon-keys/il-ewma.json`
- `/root/halcyon-keys/sol-ewma.json`
- `/root/halcyon-keys/flagship-ewma.json`
- `/root/halcyon-keys/flagship-sigma.json`

Role notes:

- `observation.json` must match the on-chain Observation keeper and is reused by
  the `write-autocall-schedule` timer.
- `delta.json` must match the on-chain Delta keeper because
  `write_aggregate_delta` authenticates against `KeeperRegistry.delta`.
- `flagship-sigma.json` must hold the same authority as `KeeperRegistry.observation`
  because `write_sigma_value` reuses the Observation keeper role.
- `regression.json` must match the on-chain Regression keeper and is reused by
  the `write-regression` job.
- `hedge.json` must match the on-chain Hedge keeper and is reused by the live
  SOL hedge keeper.
- `flagship-hedge.json` also authenticates against `KeeperRegistry.hedge`.
  It can be a copy of `hedge.json`, but the service is scaffold / dry-run only
  in this repo and should stay masked unless you are intentionally testing it.
- `regime.json` must match the on-chain Regime keeper and is reused by the
  `fire-reduced-ops` timer.
- `il-ewma.json`, `sol-ewma.json`, and `flagship-ewma.json` are fee-payer keys
  only; `update_ewma` is permissionless.
- `price-relay.json` is not in `KeeperRegistry`; it only pays for Pyth relay
  transactions.
