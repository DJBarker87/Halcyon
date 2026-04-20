# Halcyon Flagship Worst-of Equity Autocall v1 — Current Candidate

## Status

This document freezes the current repo-facing flagship structure and the disclosure text that should travel with any devnet or mainnet launch review.

It also closes one documentation gap from Layer 5: the flagship delta keeper should not be described as a heuristic estimate when the shipped path is analytical.

---

## Product family

**Worst-of SPY / QQQ / IWM autocallable note**

- Underlyings: SPY, QQQ, IWM
- Settlement: USDC cash-settled
- Tenor: 378 trading days, roughly 18 months
- Coupon cadence: monthly
- Autocall cadence: quarterly
- Coupon barrier: 100% of initial worst-of level
- Autocall barrier: 100% of initial worst-of level
- Knock-in barrier: 80% of initial worst-of level
- Knock-in monitoring: daily close
- Coupon memory: yes
- Hedge control: off-chain delta keeper plus hedge keeper

This is a **worst-of autocall**, not a principal-protected yield product.

---

## Current v1 structure

### Core terms encoded in the product program

- **Monthly coupon observations:** 18
- **Quarterly autocall observations:** 6
- **Coupon barrier:** 100%
- **Autocall barrier:** 100%
- **Knock-in barrier:** 80%
- **Engine version:** 1

### Pricing posture

- The flagship quote path is deterministic Rust
- The product uses compile-time correction tables for the K=12 filter correction and the daily knock-in cadence correction
- Entry, coupon, observation, knock-in, and settlement state live on-chain in `FlagshipAutocallTerms`
- Quote preview comes from the live on-chain `preview_quote` handler

---

## Plain-English user proposition

Earn monthly coupons while the worst performer among SPY, QQQ, and IWM stays at or above its initial level, with quarterly early redemption if the basket is strong enough.

- Deposit USDC
- Each month, the note checks the worst performer against the coupon level
- Each quarter, the note also checks whether the note autocalled
- If the basket stays healthy enough, coupons are paid and the note may redeem early
- If the basket breaches the knock-in barrier and finishes weak, principal is at risk

This is for users who are:

- neutral to moderately bullish on large-cap US equities
- comfortable giving up upside beyond the coupon profile
- willing to take worst-of downside risk in stressed markets

---

## Payoff summary

### During the life of the note

Three schedules matter:

- **monthly coupons**
- **quarterly autocall checks**
- **daily knock-in monitoring**

### If the monthly coupon condition is met

- the note pays the scheduled coupon
- missed coupons remain in memory until a later qualifying observation

### If the quarterly autocall condition is met

- the note redeems early
- principal returns in USDC
- accrued coupon memory is handled according to the on-chain policy state

### If the note reaches maturity without autocalling

- if knock-in was never triggered, principal returns at par in USDC
- if knock-in was triggered and the final worst performer is below initial, redemption is reduced in line with final worst-of performance

---

## Delta and hedge methodology disclosure

The flagship hedge control path relies on an off-chain `delta_keeper` that computes per-note and aggregate delta from live on-chain terms plus live Pyth SPY, QQQ, and IWM prices.

That path should be described precisely:

- it uses the analytical `quote_c1_filter_with_delta_live` gradient path
- it is **not** a placeholder estimate
- it is **not** a Monte Carlo hedge engine

Validation posture:

- the core `triangle_probability_with_grad` primitive is Stein-validated against the shipped expectation path
- the higher-level pricer path has finite-difference sanity coverage in the quote crate
- the aggregate output is committed on-chain through `AggregateDelta` as a Merkle root over per-note deltas

Auditability posture:

- an auditor can recompute per-note deltas from `FlagshipAutocallTerms` plus the recorded Pyth spot snapshot
- the Merkle root makes the published aggregate auditable
- this improves reviewability relative to a black-box Monte Carlo keeper

This does **not** remove operational risk. The delta path is still off-chain, so stale feeds, bad config, keeper bugs, or poor operator procedures can still produce stale or wrong hedge inputs.

---

## Risk disclosure

This is **not** a savings product and it is **not** principal protected.

Users can lose capital if:

- any of SPY, QQQ, or IWM breaches the 80% knock-in barrier during the life of the note, and
- the worst performer finishes below its initial level at maturity

Users also take structure-specific risk:

- this is a **worst-of** payoff, so the weakest name drives downside
- monthly coupons can be missed in weak markets
- upside is capped by the coupon profile and autocall
- the hedge path depends on off-chain keepers, xStocks availability, Jupiter execution quality, and live oracle health

The hedge methodology is analytical and auditable, but it is still an operational dependency rather than an on-chain guarantee.

---

## Next workstreams

1. final legal term-sheet wording and user risk legend
2. explicit mainnet geoblock / KYC decision before exposure
3. external audit review of the flagship delta and hedge control path
4. live feed verification for SPY, QQQ, and IWM receiver accounts before unpausing
5. operator evidence capture for Merkle artifact publication and independent recomputation
