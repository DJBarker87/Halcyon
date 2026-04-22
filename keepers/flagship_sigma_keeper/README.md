# Flagship Sigma Keeper

This keeper computes the flagship `sigma_common` off-chain and posts it into
the kernel's `VaultSigma` account through `halcyon keepers write-sigma-value`.

Methodology:

- daily SPY closes from Pyth Benchmarks, using the same SPY feed lineage as the
  on-chain protocol
- RiskMetrics EWMA on SPY log returns with `decay = 0.94`
- 45-day rolling mean smoother over the annualised EWMA series
- annualise on a 252-trading-day basis
- transform to `sigma_common` via the frozen shipping factor model:
  `sqrt(SPY_ewma_vol^2 - Sigma_eps[0,0] * 252) / ell_spy`

Cross-check:

- The script verifies the fixed-date target `2026-04-10`
- Reference `sigma_s6 = 248703`
- Exact match is ideal; a residual drift below `1%` is accepted to allow for
  small Yahoo/Pyth series differences during migration
- If the drift is `>= 1%`, the script exits non-zero and does not submit

The current on-chain `Regression` account does not store `ell_spy` or
`Sigma_eps[0,0]`, so those two values are pinned from the checked-in full-sample
factor-model artifact used by the shipping flagship quote crate.

## v1.1 TODO

Switch the keeper off Pyth Benchmarks and onto the local relay-cache once the
relay has accumulated enough SPY history. Historical simulation shows the
bootstrap drift falling from `0.641%` at switch time to `0.240%` after one
month, `0.148%` after two months, and `0.025%` after three months when the
post-switch daily closes come from the relay-cache path.
