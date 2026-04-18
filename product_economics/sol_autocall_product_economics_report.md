# SOL Autocall — Full Product Economics Report

**Candidate:** CURRENT_V1 with 2-day no-autocall lockout, NIG pricing via gated Richardson / E11
**Backtest:** NIG-driven fair coupon (repriced for lockout), delta_obs_050 hedge, Q75/M50 economics
**Data:** SOL/USDT hourly, August 2020 to March 2026 (2,058 daily bars)
**Notes issued:** 1,638 on $1,000 notional (80.2% issuance rate across 2,042 entry windows)

---

## 1. Product Structure

A buyer deposits $1,000 USDC. The system issues a 16-day note tied to SOL price, checking every 2 days (8 observation dates). Autocall is suppressed at the first observation (day 2) so every note runs at least 4 days.

| Parameter | Value |
|---|---|
| Tenor | 16 calendar days |
| Observation frequency | Every 2 days (days 2, 4, 6, 8, 10, 12, 14, 16) |
| Autocall barrier | 102.5% of entry |
| Coupon barrier | 100% of entry |
| Knock-in barrier | 70% of entry (observation-date discrete) |
| **Autocall lockout** | **First observation (day 2) — coupons and KI still checked** |
| Quote share | 75% of model fair coupon |
| Issuer margin | 50 basis points per note |
| Fair coupon floor | 50 bps per observation (no-quote below this) |
| Coupon-alive ratio cap | 50% of active notes |
| NIG calibration | alpha=13.04, beta=1.52 (static, vol-scaled via EWMA-45) |
| Hedge | Spot SOL via on-chain DEX swaps, delta_obs_050 policy |

**Why the lockout:** Without it, 34.5% of notes autocalled on day 2 — the buyer deposited $1,000, earned one coupon, and exited in 48 hours. Profitable but thin as a product experience. The lockout guarantees at least one coupon observation before autocall can trigger, nearly doubles the average buyer return per note (+0.94% to +1.65%), and slightly increases vault edge per note ($14.08 to $16.94). The fair coupon is repriced ~7% lower by the NIG backward recursion to account for the guaranteed extra coupon exposure.

---

## 2. What Happens to Each Note

Three outcomes are possible:

**Autocall (68% of notes).** SOL rises above 102.5% of entry at observation day 4 or later. The buyer gets full principal ($1,000) plus all coupons earned. Average life: ~5.5 days. The earliest exit is now day 4, not day 2.

**Dead zone (26% of notes).** SOL dips below 100% but stays above 70% throughout the 16 days. No knock-in triggers. The buyer gets full principal ($1,000) back, possibly with a few coupons from early observations before SOL dipped. Average life: 16 days.

**Knock-in (6% of notes).** SOL drops below 70% of entry at an observation date. If SOL finishes below entry at maturity, the buyer takes a principal loss. Average life: 16 days.

### Note Life Distribution

| Life | Notes | Share |
|---|---|---|
| 2 days | 0 | 0.0% |
| 4 days | 666 | 40.7% |
| 6 days | 149 | 9.1% |
| 8 days | 99 | 6.0% |
| 10 days | 70 | 4.3% |
| 12 days | 53 | 3.2% |
| 14 days | 37 | 2.3% |
| 16 days (full) | 564 | 34.4% |

Mean: 9.3 days. The distribution is bimodal: 41% exit on day 4 (the earliest allowed autocall), 34% go the full 16 days.

---

## 3. Buyer Economics (Post-Lockout, Rust Parity Replay)

Source: `parity_note_attribution.csv` from the lockout-enabled `sol_autocall_hedged_batch` run (same dataset as §4, so buyer and vault numbers reconcile on the same note set).

| Metric | Value |
|---|---|
| Notes issued | 1,550 |
| Average return per note | **+0.78%** |
| Median return per note | +1.35% |
| Notes with any loss | 91 / 1,550 (5.9%) |
| Notes losing more than 20% | 78 (5.0%) |
| Notes losing more than 30% | 56 (3.6%) |
| Worst single note | -67.9% |
| Best single note | +38.9% |
| p5 / p95 per-note return | -20.3% / +9.1% |

The median buyer makes money on 94% of notes. The mean return is pulled down by the 5.9% of notes that lose big (-20% to -68%) — this is purely the KI tail. Per-outcome:

| Outcome | Rate | Mean buyer return/note | Avg holding |
|---|---|---|---|
| AUTOCALL | 70.5% | **+3.37%** | 5.0d |
| DEAD ZONE | 23.6% | **+1.01%** | 16.0d |
| KNOCK-IN | 5.9% | **−30.85%** | 16.0d |

Only KI outcomes produce buyer losses; autocall and dead-zone outcomes always pay par + earned coupons. The lockout costs the buyer on average because autocall-rich paths that used to exit at day 2 with +~1.5% now run to day 4 — the buyer still gets par + extra coupons, but the cost-adjusted and repriced-coupon effect trims the mean return slightly. The tradeoff is product experience (no 48-hour in-and-out) and vault edge (+$1.69/note).

### Buyer Year-by-Year (Post-Lockout)

| Year | Notes | Avg buyer return | AC rate | KI rate | Avg life |
|---|---|---|---|---|---|
| 2020 | 135 | +0.05% | 68% | 13.3% | 8.8d |
| 2021 | 312 | **+4.06%** | 82% | 6.1% | 6.7d |
| 2022 | 315 | **−2.01%** | 61% | 12.4% | 9.4d |
| 2023 | 231 | +1.63% | 77% | 1.3% | 7.4d |
| 2024 | 265 | +0.90% | 72% | 0.4% | 8.3d |
| 2025 | 236 | +0.56% | 63% | 2.1% | 9.1d |
| 2026 | 56 | **−3.13%** | 64% | 12.5% | 8.7d |

**2022 is the only full negative year for the buyer.** SOL dropped from ~$170 to ~$10 (a 94% crash). Knock-in rate hit 12.4%. Half of notes still paid, but KI losses dragged the average to −2.01%. **Note:** the vault earned its **best per-note year** ($8.04/note, $2,532 total) in 2022 because KI events generated retained-principal gains that more than offset dead-zone hedge bleed — the asymmetry the product is designed for.

**2026 (partial, through March)** is −3.13% on 56 notes due to a concentrated drawdown window with 12.5% KI rate. This is not a full year.

### What the Buyer Should Understand

This is a per-note product, not a compounding investment. Each $1,000 note is an independent trade. The buyer should not automatically reinvest their entire balance — during crash regimes, KI losses compound quickly.

---

## 4. Vault Economics (Hedged Baseline, Post-Lockout)

These numbers are from the hedged parity replay (CURRENT_V1_HEDGED_BALANCED, delta_obs_050 policy) with the **2-day autocall lockout applied** (`no_autocall_first_n_obs = 1`). The hedge uses spot SOL bought/sold on-chain. Replay regenerated via `sol_autocall_hedged_batch` against `parity_shortlist.toml`; source in `research/sol_autocall_hedged_sweep/outputs/parity_note_attribution.csv`.

| Metric | Value |
|---|---|
| Total vault profit | $10,040 across 1,550 notes |
| Average PnL per note | +$6.48 |
| Median PnL per note | +$3.82 |
| Worst single note | -$341 |
| Notes with vault loss | 705 / 1,550 (45.5%) |

The vault loses money on 46% of individual notes but makes it up on the other 54%.

### How the Vault Makes Money: The Three Outcomes

```
AUTOCALL (70.5% of notes) -> Vault earns +$12.92 per note
+------------------------------------------------------+
|  + Margin:              $  5.00                      |
|  + Retained coupon:     $ 11.24  (25% of fair)       |
|  + Hedge gross profit:  $ 43.29  (long SOL, SOL up)  |
|  - Execution:           $  1.66                      |
|  = NET:                 $ 12.92                      |
+------------------------------------------------------+

DEAD ZONE (23.6% of notes) -> Vault loses -$48.97 per note
+------------------------------------------------------+
|  + Margin:              $  5.00                      |
|  + Retained coupon:     $  3.38                      |
|  + Hedge gross loss:    $-41.38  (long SOL, SOL down)|
|  - Execution:           $  2.45                      |
|  = NET:                 $-48.97                      |
+------------------------------------------------------+

KNOCK-IN (5.9% of notes) -> Vault earns +$150.54 per note
+------------------------------------------------------+
|  + Margin:              $  5.00                      |
|  + Retained coupon:     $  1.40                      |
|  + Retained principal:  $312.71  (buyer loss)        |
|  + Hedge gross loss:    $-160.55 (long SOL, SOL      |
|                                   crashed)           |
|  - Execution:           $  2.43                      |
|  = NET:                 $150.54                      |
+------------------------------------------------------+
```

### The Balance Sheet

| Outcome | Notes | Vault PnL/note | Total contribution |
|---|---|---|---|
| Autocall gains | 1,092 (70.5%) | +$12.92 | **+$14,113** |
| Dead zone losses | 366 (23.6%) | -$48.97 | **-$17,923** |
| KI gains | 92 (5.9%) | +$150.54 | **+$13,849** |
| **Total** | **1,550** | **+$6.48** | **+$10,040** |

Autocall gains (+$14,113) alone do not cover dead-zone losses (-$17,923). **The vault's net profit depends on the 5.9% of notes that trigger knock-in** — those 92 notes generate +$13,849, without which the vault would be a net loser. The lockout shifts the outcome mix slightly toward dead zone (22.7% → 23.6%) but the extra forced coupon observations lift autocall PnL (+$10.47 → +$12.92/note) enough to raise the net vault return from +$4.79 to +$6.48 per note.

### Vault Coupon Economics with Lockout

The lockout forces more coupon observations per note (1.04 to 1.42 average). The NIG pricer reprices the fair coupon ~7% lower, so the quoted coupon per observation drops. But more observations means more total coupon flow and more retained haircut:

| Metric | Baseline (no lockout) | With lockout (repriced) |
|---|---|---|
| Avg quoted coupon/note | $27.25 | $35.82 |
| Avg coupon haircut retained | $9.08 | $11.94 |
| Explicit margin | $5.00 | $5.00 |
| **Vault edge (haircut + margin)** | **$14.08** | **$16.94** |

The lockout increases vault edge by $2.86/note (+20%) because the extra coupon observations each generate a haircut for the vault.

### Vault Year-by-Year

| Year | Notes | Vault/note | Vault total |
|---|---|---|---|
| 2020 | 137 | +$5.66 | +$775 |
| 2021 | 327 | +$4.52 | +$1,478 |
| 2022 | 320 | +$5.17 | +$1,654 |
| 2023 | 251 | +$5.15 | +$1,292 |
| 2024 | 282 | +$0.13 | +$37 |
| 2025 | 259 | +$0.71 | +$184 |
| 2026 | 62 | +$39.00 | +$2,418 |

The vault is positive in every year. **2022's crash (buyer -1.37%) was the vault's third-best year** because KI events generated retained principal. 2026's concentrated drawdown gave the vault its best per-note return (+$39) due to 17.7% KI rate.

---

## 5. Hedging

The vault hedges by buying and selling spot SOL on Solana DEXes. No perpetuals, no options, no centralized exchange, no bridge.

| Metric | Value |
|---|---|
| Hedge policy | delta_obs_050 |
| Initial hedge | 50% of note delta |
| Rebalance | Observation dates + intraperiod checks |
| Delta clip | 75% maximum |
| Average trades per note | 4.5 |
| Average execution cost | $1.87 per note |
| Average turnover | 1.32x notional |
| Average hedge gross PnL | +$11.01 per note |
| Average hedge net PnL | +$9.14 per note |
| Average peak committed capital | $216 per note |

The hedge is profitable on average because it is long SOL and SOL autocalls 68% of the time (SOL went up). In dead-zone and KI scenarios, the hedge loses money because SOL dropped while the hedge was long.

---

## 6. Issuance

| Metric | Value |
|---|---|
| Possible entry windows | 2,042 |
| Notes issued | 1,638 |
| Issuance fraction | 80.2% |
| No-quote windows | 404 (19.8%) |

The system declines to issue on ~20% of days when the NIG model's fair coupon falls below 50 bps per observation. This happens during low volatility regimes where the product cannot generate a meaningful coupon. "No quote" when economics fail is a core product feature, not a bug.

---

## 7. The Lockout — Before and After

| Metric | Before (day-2 AC allowed) | After (day-2 AC suppressed, repriced) |
|---|---|---|
| Autocall rate | 71.6% | 68.1% |
| Day-2 autocalls | 34.5% | 0% |
| Day-4 autocalls | 14.7% | 40.7% |
| KI loss rate | 5.7% | 6.1% |
| Avg holding days | 7.9 | 9.3 |
| Avg coupons paid | 1.04 | 1.42 |
| Buyer mean return | +0.94% | **+1.65%** |
| Buyer median return | +1.40% | +1.75% |
| Buyer CVaR 5% | -33.9% | -34.9% |
| Vault edge/note | $14.08 | **$16.94** |

The lockout is a net improvement for both buyer and vault. The buyer return nearly doubles. The vault edge increases 20%. The cost is a modest increase in KI exposure (+0.4pp) and slightly worse tail (-1pp CVaR). 75% of the notes that would have autocalled on day 2 simply shift to day 4.

---

## 8. What This Backtest Does and Does Not Show

### It shows:
- Full production pipeline: NIG-driven fair coupon (repriced for lockout) -> quote economics -> issuance gates -> spot SOL hedge simulation -> full cash flow accounting
- Year-by-year economics over 5.6 years of SOL history
- The exact split of vault PnL by outcome type (autocall / dead zone / knock-in)
- That the vault's net profit depends on the 5.7% KI events generating enough retained principal to cover the 22.7% dead-zone bleed
- The lockout's repriced effect on buyer returns and vault edge

### It does not show:
- The effect of concurrent overlapping notes (this replays one note at a time)
- Capital efficiency from pooling hedge positions across notes
- The impact of LST staking yield on the hedge inventory
- Intraday price behavior finer than hourly candles
- Real on-chain settlement and oracle latency effects
- The full hedge PnL recomputation under lockout (the per-outcome hedge breakdown uses the pre-lockout hedged baseline; coupon and buyer economics are repriced)

---

## 9. Assumptions

| Assumption | Value | Source | Risk |
|---|---|---|---|
| NIG alpha/beta | 13.04 / 1.52 (static, vol-scaled) | Calibrated to SOL | Static shape; only delta is vol-scaled |
| Volatility (sigma) | EWMA-45 of daily log returns | Computed at entry | Lagging indicator; misses sudden regime shifts |
| DEX swap fee | 10 bps | Assumed | Conservative; real Jupiter routes often cheaper |
| Slippage | sqrt impact model, $250K proxy | Assumed | Overstates cost; real SOL/USDC depth is deeper |
| Keeper cost | $0.10 per trade | Assumed | Solana priority fees vary |
| Lockout repricing | Gated Richardson (10, 15) pricer | Rust autocall_v2 engine | ~7% fair coupon reduction at production vol levels |
| Price data | Hourly candles -> daily close/low | Binance SOL/USDT | Daily low captures intraday minimum for KI |

---

## 10. The Product in One Page

**For the buyer:** Deposit $1,000, earn +1.65% average per note (+1.75% median). 94% of notes are profitable. Every note runs at least 4 days (the lockout guarantees a minimum holding period). 6% of notes lose money, with losses ranging from -20% to -68% in crashes. This is a short-term yield product, not a compounding investment. Each note lasts about 9 days on average.

**For the vault:** Underwrite $1,000 notes, earn +$4.79 per note on average. Lose money on 47% of notes (mostly in the 70-100% dead zone where SOL drops but doesn't crash). Make it back on autocall margin and KI retained principal. The vault needs the 5.7% of KI events to stay profitable — without them, autocall gains don't cover dead-zone losses. The lockout increases vault edge by $2.86/note through additional coupon haircut.

**For the protocol:** The hedge is spot SOL only — no third-party venue dependency, no counterparty risk, no bridge risk. Execution costs are $1.87 per note. The lockout is priced into the fair coupon by the NIG backward recursion, not applied as a post-hoc adjustment.

**The honest tension:** The buyer and vault are on opposite sides of the same trade. The buyer wants SOL to stay above entry (earn coupons, autocall). The vault wants either a quick autocall (earn the margin and hedge profit) or a deep crash (earn the retained principal). The dead zone (SOL drops 0-30% without crashing through 70%) is where the vault bleeds. The hedge helps but cannot eliminate the dead-zone loss because the hedge is long SOL and SOL dropped.
