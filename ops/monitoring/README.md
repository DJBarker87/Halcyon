# Halcyon Monitoring

Layer 5 minimum monitoring requirements:

- RPC health per endpoint used by keepers
- keeper heartbeat and last-success timestamps
- Pyth feed freshness for SPY, QQQ, IWM, SOL, USDC
- vault utilization
- policy-state summary
- alert on failed settlement paths

## Suggested stack

- Prometheus
- Alertmanager
- Grafana
- blackbox exporter for RPC probing
- log shipping from JSON keeper stdout

## Metric contracts

Use these metric names consistently across exporters and dashboards:

- `halcyon_keeper_last_run_timestamp_seconds{keeper="..."}`
- `halcyon_keeper_last_success_timestamp_seconds{keeper="..."}`
- `halcyon_keeper_consecutive_failures{keeper="..."}`
- `halcyon_pyth_feed_age_seconds{feed="SPY|QQQ|IWM|SOL|USDC"}`
- `halcyon_vault_utilization_ratio`
- `halcyon_policy_count{product="...",status="..."}`
- `halcyon_apply_settlement_failures_total{product="..."}`

## RPC probes

Run blackbox probes against every keeper RPC endpoint. Alert if:

- primary endpoint is down
- both primary and failover are down
- latency or error rate spikes enough to threaten keeper cadence

## Keeper heartbeats

Each keeper process should emit heartbeat timestamps either through:

- a tiny exporter sidecar, or
- structured logs transformed into metrics

Alert if any keeper exceeds 2x its expected cadence:

- observation keeper
- hedge keeper
- regime keeper
- regression keeper
- delta keeper

## Feed freshness

Alert when any feed age exceeds its configured cap:

- SPY
- QQQ
- IWM
- SOL
- USDC

## Utilization

Alerting thresholds:

- warning at `> 0.85`
- critical near protocol cap or if cap is crossed

## Settlement failures

Treat any increment of `halcyon_apply_settlement_failures_total` as page-worthy until the launch period stabilizes.

## Files in this directory

- `prometheus/halcyon_alerts.yml`: alert template

This repo does not yet ship a live exporter binary. The files here are the launch contract for the metrics surface operations should stand up.
