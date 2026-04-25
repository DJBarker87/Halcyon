# Flagship Midlife Pricer Report

**Product:** SPY / QQQ / IWM worst-of flagship autocall  
**Use case:** live on-chain NAV, lending-value preview, and buyback / liquidation pricing  
**Current status:** production path wired through checkpointed on-chain pricing  
**Generated:** 2026-04-24

This report documents the midlife NAV pricer used after issuance, while a note is live. The important point is that this is not a fixture correction system. The on-chain path runs the deterministic C1-filter monthly dynamic program from live policy state and fresh oracle inputs, checkpointed across instructions when the compute budget is too small for a single transaction.

## Executive Summary

The flagship note needs a live NAV because lending protocols cannot accept a structured note as collateral if the only mark is an issuer-signed off-chain number. The midlife pricer provides that mark on-chain.

The current implementation:

- Recomputes NAV from live Pyth SPY / QQQ / IWM prices, the stored note terms, the current vault sigma, and the stored regression state.
- Uses the same fixed-point C1-filter family as the issuance pricer, specialised for the live monthly coupon / quarterly autocall state.
- Handles coupon memory, KI latch state, remaining quarterly autocall dates, remaining monthly coupon dates, and maturity redemption.
- Splits the computation through a temporary checkpoint account when one transaction is not enough.
- Feeds both `preview_lending_value_from_checkpoint` and `buyback_from_checkpoint`.

Latest validation results:

| Check | Result |
|---|---:|
| Full production-grid parity sweep | 300 / 300 passing |
| Full sweep exact matches | 260 / 300 |
| Full sweep max absolute NAV diff | 917 s6 = 9.17 bps |
| Full sweep p95 absolute NAV diff | 12 s6 = 0.12 bps |
| Full sweep worst understatement | -60 s6 = -0.60 bps |
| Full sweep CU overflows | 0 |
| Worst observed per-transaction CU | 1,275,024 |
| Hard-cap headroom vs 1.4M CU | about 124,976 CU |
| Tail resume from fixture index 225 | 75 / 75 passing |

Source note: `research/midlife_parity_report.json` currently contains the resumed tail run from fixture index 225 rather than the full 300-case sweep. The full 300-case figures above are from the latest complete integration run log. The checked-in tail artifact shows 75 / 75 passing, max absolute diff 7 s6, p95 0 s6, max transaction CU 1,275,024, and 0 CU overflows.

## What It Prices

The midlife pricer values the remaining liability of an active flagship note per $1 notional:

```text
nav = PV(remaining coupons) + PV(redemption)
```

The output is returned in scale-6 fixed point:

```text
1_000_000 = 1.000000 = par
```

The on-chain output bundle is:

| Field | Meaning |
|---|---|
| `nav_s6` | Present value of the remaining payoff per $1 notional |
| `ki_level_usd_s6` | Notional-denominated KI cap used by lending-value / buyback formulas |
| `remaining_coupon_pv_s6` | Diagnostic PV of unpaid future coupon liability |
| `par_recovery_probability_s6` | Diagnostic probability of ending at par |
| `sigma_pricing_s6` | Pricing sigma used for the run |
| `now_trading_day` | Trading-day age of the note used by the pricer |

The lending value then applies the conservative liquidation haircut:

```text
lending_value_s6 = max(0, min(current_nav_s6 - 100000, ki_level_s6 - 100000))
payout_usdc      = policy_notional_usdc * lending_value_s6 / 1000000
```

The 100000 term is the 10% haircut at scale 6. In healthy states, the lending mark is capped at `KI level - 10%`. In stressed states, the mark follows NAV down with the same haircut.

## Product Backtest Context

The midlife pricer exists because the flagship economics depend on a note being liquidatable while live. The historical economics are documented in `product_economics/worst_of_autocall_product_economics_report.md`.

Backtest setup:

| Item | Value |
|---|---|
| Underlyings | SPY, QQQ, IWM |
| Data | Daily bars, April 2006 to April 2026 |
| Model | One-factor NIG with Gaussian residuals |
| Calibration | 77 quarterly calibrations, each on trailing 252 trading days |
| Product | 18-month worst-of autocall |
| Coupon observations | Monthly, 18 dates |
| Autocall observations | Quarterly, 6 dates |
| Coupon / autocall barrier | 100% of entry |
| KI barrier | 80% of entry |
| Coupon policy | Memory coupon |
| Quote share | 65% of model fair coupon |
| Fair coupon gate | 50 to 500 bps per observation |
| Junior first loss | 12.5% of notional |

Historical issuance-and-settlement replay:

| Metric | Monthly cadence | Daily cadence |
|---|---:|---:|
| Possible entry windows | 210 | 4,400 |
| Notes issued | 206 | 4,291 |
| Issuance rate | 98.1% | 97.5% |
| Buyer annualised IRR | +8.35% | +8.49% |
| Annualised quoted coupon | 14.9% | 15.1% |
| Autocall rate | about 83% | 83.1% |
| KI trigger rate | about 22% | 21.7% |
| Principal-loss rate | about 13% | 14.4% |
| Average note life | about 145 trading days | 145 trading days |

Vault economics from the production daily row:

| Metric | Value |
|---|---:|
| Vault occupied-capital return | +5.63% |
| Mean vault P&L per note | $3.82 |
| Worst single-note vault P&L | -$18.87 |
| Peak concurrent notes | 317 |
| Worst cumulative drawdown | -$265.5 |
| Drawdown as % of peak concurrent book | about 0.84% |
| Insolvency events | 0 |

Mechanism-active buyback overlay:

| Scenario | Liquidations | Failures | Min coverage | Worst single day |
|---|---:|---:|---:|---:|
| Primary lending liquidation | 452 | 0 | 1.3319x | 107 buybacks |
| Forced stress liquidation | 708 | 0 | 1.2458x | 73 buybacks |

The buyback overlay is the economic reason the midlife pricer matters. It showed that, under the current research hedge-unwind model, liquidation buybacks were always payable from the note's own dedicated balance sheet plus its reserved support. The remaining production risk is real wrapper liquidity / basis during stress, not whether the protocol can compute the liquidation mark.

## Validation Data

### Fixture Grid

The committed host-reference fixture file is:

```text
crates/halcyon_flagship_quote/tests/fixtures/midlife_nav_vectors.json
```

It contains 300 vectors:

| Dimension | Coverage |
|---|---|
| Pricing reference | `nav_c1_filter_mid_life` |
| Quadrature | GH9 |
| State families | healthy 90, near-KI 90, post-KI moderate 72, post-KI severe 36, edge 12 |
| Sigma points | 100000, 180000, 280000 s6 |
| Coupon indices | 0, 3, 6, 9, 12, 17 |
| KI-latched states | 114 / 300 |
| Not-yet-KI states | 186 / 300 |
| Missed coupon buckets | 0, 1, 2, 3 |
| Expected NAV range | 60,578 s6 to 1,146,917 s6 |

The fixture set intentionally covers:

- Healthy ITM states above the coupon / autocall barrier.
- ATM states around 100%.
- Near-KI states around 79% to 83%.
- Post-KI states where redemption follows the worst performer.
- Terminal / edge cases at coupon 17.

### Full SBF Sweep

Latest full-sweep result from the integration run:

| Metric | Value |
|---|---:|
| Fixtures | 300 |
| Passing | 300 |
| Failing | 0 |
| CU exceeded | 0 |
| Exact matches | 260 |
| Overstated cases | 4 |
| Understated cases | 36 |
| Max signed diff | +917 s6 |
| Min signed diff | -60 s6 |
| Max absolute diff | 917 s6 |
| p95 absolute diff | 12 s6 |
| Worst per-transaction CU | 1,275,024 |

Interpretation:

- The largest error is positive, so the worst tail is vault-disadvantageous: it slightly overpays relative to the host reference.
- The understatement tail is broader by count but small by magnitude: worst -60 s6, or -0.60 bps.
- p95 is effectively exact at 12 s6, or 0.12 bps.
- The current worst CU case has about 125k CU of headroom below Solana's 1.4M transaction limit.

### Tail Resume Artifact

The checked-in artifact currently at `research/midlife_parity_report.json` is the resumed tail run:

| Metric | Value |
|---|---:|
| Fixtures | 75 |
| Passing | 75 |
| Failing | 0 |
| CU exceeded | 0 |
| Exact matches | 72 |
| Understated cases | 3 |
| Max absolute diff | 7 s6 |
| p95 absolute diff | 0 s6 |
| Mean absolute diff | 0.2 s6 |
| Max per-transaction CU | 1,275,024 |
| p95 per-transaction CU | 1,209,994 |

Worst tail cases in that resumed run:

| Label | Signed diff | Transaction CU | Transaction count | Chunk size |
|---|---:|---:|---:|---:|
| `healthy/r120/sigma=280000/coupon=6` | -7 | 1,045,340 | 30 | 1 |
| `healthy/r120/sigma=280000/coupon=9` | -6 | 1,045,340 | 16 | 1 |
| `healthy/r120/sigma=280000/coupon=12` | -2 | 808,807 | 8 | 1 |

### Checkpoint Identity Tests

The host Rust tests include two important invariance checks:

- Every committed vector is evaluated one-shot and through checkpoint splits.
- Held-out generated states are created by perturbing spot, sigma, residual vol, and coupon inputs; those states are not fixture values, and checkpointed execution must still match one-shot execution exactly.

The held-out checkpoint test is what prevents the validation from being circular. The checkpoint machinery is not retrieving fixture values; it preserves the same deterministic dynamic-program state across arbitrary split boundaries.

## How It Works On-Chain

The production path is split into three instruction families.

### 1. Prepare

```text
prepare_midlife_nav(policy, terms, regression, vault_sigma, pyth_spy, pyth_qqq, pyth_iwm, checkpoint, stop_coupon_index)
```

Prepare does the live input capture:

1. Checks policy and product terms are active.
2. Checks vault sigma and regression freshness.
3. Reads fresh Pyth SPY / QQQ / IWM prices in the transaction.
4. Composes the pricing sigma from vault sigma, protocol floor, and protocol ceiling.
5. Computes note age in trading days.
6. Builds `MidlifeInputs`.
7. Runs the C1-filter DP from the current coupon index up to `stop_coupon_index`.
8. Writes the deterministic frontier into the checkpoint account.

The checkpoint account contains:

| Component | Purpose |
|---|---|
| Magic/version | Rejects malformed accounts |
| Requester | User that prepared the checkpoint |
| Policy and terms pubkeys | Binds checkpoint to one note |
| Prepared and expiry slots | Prevents stale checkpoint reuse |
| Full input snapshot | Freezes the market/input state used for this valuation |
| Frontier bytes | DP state: safe and knocked memory buckets, weights, means, totals |

The checkpoint account size is 20,706 bytes and expires after 512 slots.

### 2. Advance

```text
advance_midlife_nav(checkpoint, stop_coupon_index)
```

Advance resumes the checkpoint from its saved cursor and processes more coupon observations. It is deterministic: the input snapshot is already stored inside the checkpoint, so it does not reread oracles or accept new market data.

This is what lets expensive states price correctly without inventing a cheaper approximation. If the note is early in life and the state frontier is wide, the client packs multiple advance instructions into as few transactions as the CU budget allows.

### 3. Finish

There are two production finish paths:

```text
preview_lending_value_from_checkpoint(checkpoint)
buyback_from_checkpoint(checkpoint, policy, terms, holder, usdc_mint)
```

Both finish paths call the same deterministic `finish_nav_from_checkpoint` routine. The preview path returns the lending-value quote. The buyback path recomputes the same value, pays the current owner or collateral holder, settles the terms, emits the buyback event, and closes the checkpoint account.

The frontend and SDK now use this flow for live borrowing and live buyback. The direct one-shot `preview_lending_value` path remains useful for cheap states and read-only display, but the signed production flow uses checkpointing.

## Client-Side Chunking

The client does not discover chunk count by submitting failing transactions. It simulates candidate chunks before send.

Candidate chunks:

```text
18, 12, 9, 6, 4, 3, 2, 1
```

Planner behavior:

1. Try the widest candidate prepare chunk that simulates under the CU target.
2. Send prepare only after a candidate passes simulation.
3. For advance, simulate packing another advance instruction into the pending transaction.
4. If adding it would exceed the soft target, send the current transaction and start a new one.
5. Finish when the checkpoint cursor reaches coupon index 18.

Current soft target:

```text
1,280,000 CU
```

The target leaves room below the hard 1.4M cap for program/runtime variance.

Observed full-sweep chunk selection from the latest run log:

| Chunk size | Count |
|---|---:|
| 18 | 154 |
| 6 | 83 |
| 3 | 3 |
| 2 | 19 |
| 1 | 41 |

The exact `r100/coupon=0` production-path reachability test priced through prepare + packed advance + finish in three transactions, with per-transaction CU around 1.17M, 1.25M, and 1.26M. That state was important because it had previously represented the early-healthy reachability risk.

## Math

### Underlying Model

The flagship model decomposes each asset return into a shared fat-tailed factor plus residual spread terms:

```text
X_i(t) = drift_i(t) + loading_i * F(t) + epsilon_i(t)
```

where:

- `F(t)` is the common NIG factor.
- `epsilon_i(t)` is Gaussian residual risk.
- The IWM leg is supported by the stored SPY / QQQ regression terms used by the flagship program.
- The common factor is integrated with GH9 quadrature and NIG importance weights.

The live state is expressed as ratios to entry:

```text
R_i(t) = S_i(t) / S_i(0)
W(t)   = min(R_SPY(t), R_QQQ(t), R_IWM(t))
```

Payoff conditions:

```text
coupon_hit(t_m)   = W(t_m) >= coupon_barrier
autocall_hit(t_q) = W(t_q) >= autocall_barrier
ki_hit            = W(t)   <= ki_barrier
```

For the current flagship:

```text
coupon_barrier   = 100%
autocall_barrier = 100%
ki_barrier       = 80%
```

### Coupon Memory

Coupon memory is an integer bucket. If a coupon observation misses, the memory bucket increments. If a later coupon observation hits, the payoff includes the current coupon plus all missed coupons, then resets memory to zero.

For a monthly coupon rate `c` and memory count `m`:

```text
coupon_due = (m + 1) * c
```

The DP therefore tracks memory buckets, not just alive/dead state. The checkpoint stores 19 memory buckets, enough for the full 18-observation tenor plus the seed bucket.

### Redemption

At a quarterly autocall date, if all assets are at or above the autocall barrier, the note redeems at par plus due coupons.

At maturity:

```text
if not ki_latched and no future KI:
    redemption = par
else:
    redemption = min(par, worst_final_ratio)
```

The NAV output is:

```text
nav_s6 = redemption_pv_s6 + remaining_coupon_pv_s6
```

### C1 Filter State

The C1 filter is a compressed dynamic program over the live distribution of future states.

Each node stores:

```text
c       = cumulative common-factor coordinate
w       = probability weight
mean_u  = residual spread mean for QQQ vs SPY
mean_v  = residual spread mean for IWM vs SPY
```

The basis is rotated into common factor plus two residual spreads. That is the matrix-operator optimisation: instead of carrying three full asset coordinates, the code carries one common coordinate and two spread-plane means. Barrier tests become shifted half-plane / triangle probability problems in the residual plane.

For each coupon observation:

1. Build the factor transition for the step length.
2. Iterate GH9 common-factor nodes.
3. For each safe and knocked memory bucket, compute coupon, autocall, KI, and continuation probabilities using the C1 half-plane geometry.
4. Accumulate coupon PV, redemption PV, and par-recovery probability.
5. Merge / prune the frontier back to at most K nodes per bucket.

Current production K:

```text
K = 15 nodes per memory bucket
```

The checkpoint stores separate safe and knocked frontiers:

```text
safe_states[19][15]
knocked_states[19][15]
```

Safe states are paths where KI has not occurred. Knocked states are paths where KI has occurred or was already latched in the policy terms.

### Fixed Point and CU Optimisation

The pricer is fixed-point throughout the on-chain path:

| Scale | Use |
|---|---|
| S6 = 1,000,000 | probabilities, ratios, NAV, sigma |
| S12 = 1,000,000,000,000 | regression coefficients and higher precision coefficients |

Hot-path multiply-at-S6 is:

```text
m6r(a, b) = a * b / 1_000_000
```

The production feature set uses the reciprocal multiply-shift implementation in `m6r_recip`, not a Q20 divide trick. The reciprocal is stored as Q62:

```text
RECIP_S6_Q62 = floor(2^62 / 1_000_000)
m6r(a, b)    = ((a * b) * RECIP_S6_Q62) >> 62
```

This avoids expensive BPF integer division in the hottest fixed-point operation. The code comment estimates roughly 100k to 300k CU saved on BPF from this change across a quote path.

Other CU controls:

- Rotated common/spread basis.
- GH9 rather than a larger quadrature.
- K=15 compressed frontier.
- Memory-bucket capping in healthy and boundary regions where tail buckets are economically irrelevant.
- Per-observation progress checkpoints so a single expensive observation can be resumed mid-loop.
- Client-side simulation before send so chunking is deterministic and does not fee-burn failed retries.

## Production Integration Surface

Program instructions:

```text
prepare_midlife_nav
advance_midlife_nav
preview_lending_value_from_checkpoint
buyback_from_checkpoint
```

SDK / CLI surface:

- `halcyon_client_sdk::flagship_autocall::create_midlife_checkpoint_account_ix`
- `prepare_midlife_nav_ix`
- `advance_midlife_nav_ix`
- `preview_lending_value_from_checkpoint_ix`
- `buyback_from_checkpoint_ix`
- `liquidate_wrapped_flagship_from_checkpoint_ixs`
- `halcyon preview-lending-value` defaults to checkpointed pricing when a keypair is available.
- `halcyon preview-lending-value --direct` keeps the legacy one-shot simulation path.

Frontend surface:

- `/lending-demo` now uses checkpointed pricing for live borrow actions.
- Live buyback / liquidation uses unwrap plus `buyback_from_checkpoint`.
- The UI displays transaction count and max CU for checkpointed live actions.

## What This Proves

The current result proves:

- The on-chain production path can compute a live flagship midlife NAV without a fixture lookup, correction table, or off-chain mark.
- Checkpointing preserves the deterministic DP state; it is not an interpolation scheme.
- The fixture grid covers the major economic state regions: healthy, ATM, near-KI, post-KI, terminal, multiple vol points, and multiple coupon-memory positions.
- The production lending/buyback surface can reach the previously problematic early healthy state (`r100/coupon=0`) under the CU cap.
- The buyback mechanism has historical solvency support under the current economic replay and stress overlay.

## What It Does Not Prove

The current result does not prove:

- That no continuous production state can exceed the observed CU maximum. The sweep is broad but discrete.
- Real stressed wrapper liquidity for SPY / QQQ token wrappers. The backtest uses a research unwind-cost model.
- That every future compiler or Solana cost-schedule change preserves the current CU headroom.
- That read-only frontend display can always price every state without signatures. Hard states need checkpoint accounts, and checkpoint accounts require signed transactions.

## Operational Recommendations

1. Preserve the checkpointed path as the production path for lending and buyback. Do not reintroduce NAV correction tables.
2. Regenerate the full 300-case parity artifact after filtered / resumed runs so `research/midlife_parity_report.json` is not left as a tail-only report.
3. Add a denser CU sweep around the high-CU state families: early healthy `r100/r103/r110`, high sigma, and coupon 0/3.
4. Keep the soft target below 1.28M CU unless a future run proves more margin is available.
5. Document transaction count to integrators. Some states are one-shot; the expensive early-life states need multi-transaction checkpointing.
6. Treat wrapper liquidity and basis as a launch calibration problem separate from mathematical NAV parity.

## Source Map

| Area | Path |
|---|---|
| Product economics | `product_economics/worst_of_autocall_product_economics_report.md` |
| Lending / buyback mechanism | `docs/flagship_lending_value_and_buyback.md` |
| Midlife quote API | `crates/halcyon_flagship_quote/src/midlife_pricer.rs` |
| Host reference symbol | `crates/halcyon_flagship_quote/src/midlife_reference.rs` |
| C1 filter implementation | `crates/halcyon_flagship_quote/src/worst_of_c1_filter.rs` |
| On-chain input/checkpoint wrapper | `programs/halcyon_flagship_autocall/src/midlife_pricing.rs` |
| Checkpoint account constants | `programs/halcyon_flagship_autocall/src/state.rs` |
| On-chain parity harness | `tests/integration/midlife_parity.spec.ts` |
| Real product integration harness | `tests/integration/real_products.spec.ts` |
| Fixture file | `crates/halcyon_flagship_quote/tests/fixtures/midlife_nav_vectors.json` |
| Current tail report | `research/midlife_parity_report.json` |
