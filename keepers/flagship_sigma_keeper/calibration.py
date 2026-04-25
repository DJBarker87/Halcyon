"""Frozen flagship sigma calibration constants.

The on-chain `Regression` account stores hedge regression coefficients
(`beta_spy`, `beta_qqq`, `alpha`, residual vol, r^2), but it does not carry
the factor-model loading `ell_spy` or the residual covariance entry
`Sigma_eps[0,0]` needed for the flagship `sigma_common` transform.

For v1 we therefore pin the same checked-in full-sample factor-model constants
the shipping quote crate already vendors in `worst_of_factored.rs`. Quarterly
recalibration is explicitly deferred.

The off-chain sigma keeper now sources daily SPY closes from Pyth Benchmarks so
the volatility input shares the same oracle lineage as the on-chain protocol.
"""

ELL_SPY = 0.515731101696962
RESIDUAL_COV_SPY_DAILY = 8.222389184226306e-06
TRADING_DAYS_PER_YEAR = 252.0
EWMA_DECAY = 0.94
EWMA_LOOKBACK_DAYS = 45
EWMA_MIN_PERIODS = max(5, EWMA_LOOKBACK_DAYS // 3)
PYTH_BENCHMARKS_BASE_URL = "https://benchmarks.pyth.network"
SPY_PYTH_FEED_ID = "0x19e09bb805456ada3979a7d1cbb4b6d63babc3a0f8e8a9509f68afa5c4c11cd5"
SPY_PYTH_SYMBOL = "Equity.US.SPY/USD"
PYTH_HISTORY_START_DATE = "2006-04-11"

# Fixed-date cross-check from Pyth Benchmarks SPY history.
CROSSCHECK_DATE = "2026-04-10"
CROSSCHECK_EXPECTED_SIGMA_S6 = 251857
CROSSCHECK_MAX_SIGMA_DRIFT_PCT = 1.0

# Provenance notes:
# - Factor-model artifact: old repo
#   halcyon-hedge-lab/output/factor_model/spy_qqq_iwm_factor_model.json
# - Shipping code uses the same frozen full-sample model in:
#   crates/halcyon_flagship_quote/src/worst_of_factored.rs
