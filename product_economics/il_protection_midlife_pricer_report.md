# IL Protection Midlife Pricer Report

**Product:** 30-day SOL/USDC constant-product LP impermanent-loss protection  
**Use case:** live on-chain NAV and conservative lending-value preview for active IL cover  
**Current status:** production one-transaction preview path wired through shifted NIG pricing  
**Generated:** 2026-04-24

This report documents the midlife pricer used after an IL protection policy has been issued. The pricer computes the current fair value of the remaining cover, but lending value is deliberately more conservative: it advances only against current intrinsic settlement value, not against future optionality.

## Executive Summary

The IL protection midlife pricer gives a live value for a 50/50 SOL/USDC LP insurance policy. It uses the same NIG European IL engine family as issuance, shifted by the live price move since entry.

The current implementation:

- Reads live SOL/USD and USDC/USD from Pyth.
- Reads active policy terms from chain.
- Computes remaining days to expiry on-chain.
- Computes current terminal IL and intrinsic payout from entry and live prices.
- Computes a shifted NIG fair value for the remaining tenor.
- Sets NAV to at least intrinsic value, capped at max cover.
- Sets lending value to 80% of intrinsic payout only.
- Returns NAV, intrinsic payout, terminal IL, current log-ratio, sigma, remaining days, and engine version.

Latest validation coverage:

| Check | Result |
|---|---:|
| Host midlife unit tests | passing |
| Expiry NAV equals intrinsic value | covered |
| No lending against future optionality | covered |
| On-chain integration issue + preview | passing |
| On-chain preview CU cap | below 1.4M in integration |

The important policy decision is that NAV and collateral value are not the same. NAV includes future insurance optionality. Lending value only recognises already-earned intrinsic settlement value.

## What It Prices

The pricer values the remaining fair value of the IL protection policy:

```text
nav = fair value of remaining capped IL payoff
```

The payoff being insured is terminal impermanent loss between deductible and cap:

```text
payout_fraction = min(max(IL - deductible, 0), cap - deductible)
```

For the current product:

```text
deductible = 1%
cap        = 7%
max cover  = 6%
```

The on-chain output bundle is:

| Field | Meaning |
|---|---|
| `nav_s6` | Fair remaining protection value per $1 insured |
| `max_cover_s6` | Cap less deductible |
| `lending_value_s6` | Conservative lendable value per $1 insured |
| `nav_payout_usdc` | Notional-scaled NAV payout |
| `lending_value_payout_usdc` | Notional-scaled lendable value |
| `terminal_il_s6` | Current terminal IL if settled now |
| `terminal_il_s12` | Same value at S12 precision |
| `intrinsic_payout_usdc` | Current settlement payout if expired now |
| `current_sol_price_s6` | Live SOL/USD oracle price |
| `current_usdc_price_s6` | Live USDC/USD oracle price |
| `current_log_ratio_s6` | Log move of SOL/USDC pair since entry |
| `sigma_pricing_s6` | Annualised sigma used for the run |
| `remaining_days` | Ceil days until expiry |
| `engine_version` | IL engine version constant |

Lending value is:

```text
intrinsic_s6     = intrinsic_payout_usdc / insured_notional_usdc
lending_value_s6 = 80% * intrinsic_s6
```

No collateral value is assigned to out-of-the-money future optionality. If the policy has positive fair NAV but no current intrinsic claim, lending value is zero.

## Product Backtest Context

The product economics are documented in `product_economics/il_protection_product_economics_report.md`.

Current product structure:

| Item | Value |
|---|---|
| Underlying | SOL/USDC 50/50 constant-product LP |
| Pool archetype | Raydium full-range CPMM |
| Tenor | 30 days |
| Settlement | European, entry price vs expiry price |
| Deductible | 1% terminal IL |
| Cap | 7% terminal IL |
| Max payout | 6% of insured notional |
| Pricing model | NIG European IL premium |
| Underwriting load | 1.10x at issuance |
| Calm pricing sigma | EWMA x 1.30, floored |
| Stress pricing sigma | EWMA x 2.00 |

Historical economics from the current IL report:

| Metric | Value |
|---|---:|
| Backtest windows | 2,027 rolling 30-day windows |
| Data window | August 2020 to February 2026 |
| Blended loss ratio | about 80% |
| Calm share | 87% |
| Stress share | 13% |
| Post-crash net insurance cost | about 2.2% annualised |
| Post-crash protected LP return at median fee APY | about +1.6% annualised |

The midlife pricer matters because an active insurance receipt can have value before expiry. A lender still should not advance against the full option value unless it is comfortable warehousing model risk, so the implemented lending mark is intrinsic-only.

## How It Works On-Chain

The production preview instruction is:

```text
preview_lending_value(
  protocol_config,
  vault_sigma,
  regime_signal,
  policy_header,
  product_terms,
  pyth_sol,
  pyth_usdc
)
```

The instruction flow:

1. Requires the policy header and product terms to be active.
2. Checks vault sigma freshness.
3. Checks regime signal freshness.
4. Reads fresh SOL/USD from Pyth.
5. Reads fresh USDC/USD from Pyth.
6. Composes pricing sigma from vault sigma, regime signal, protocol floor, and protocol ceiling.
7. Computes remaining days to expiry as ceil seconds divided by `SECONDS_PER_DAY`.
8. Builds `IlProtectionMidlifeInputs`.
9. Calls `price_midlife_nav`.
10. Returns the `LendingValuePreview` struct.

There is no keeper matrix and no checkpoint account for IL midlife pricing. The model fits in one transaction because it is a one-period European valuation over the remaining tenor.

## Math

The current pair move is:

```text
pair_ratio_0 = entry_sol_price / entry_usdc_price
pair_ratio_t = current_sol_price / current_usdc_price
x_t          = ln(pair_ratio_t / pair_ratio_0)
```

For a 50/50 CPMM LP, terminal impermanent loss is computed by the same settlement engine used at expiry. The report-level shorthand is:

```text
IL(price_ratio) = 1/2 * (sqrt(price_ratio) - 1)^2
```

The program calls:

```text
compute_settlement_from_prices(...)
```

to compute:

```text
terminal_il_s12
intrinsic_payout_usdc
```

The intrinsic payout is converted to a fraction of insured notional:

```text
intrinsic_s6 = intrinsic_payout_usdc / insured_notional_usdc
```

If no time remains:

```text
fair_nav_s6 = intrinsic_s6
```

Otherwise, the shifted NIG engine prices the remaining payoff conditional on the current log move:

```text
fair_nav_s6 = nig_european_il_premium_shifted(
  sigma_annual_s6,
  remaining_days,
  deductible_s6,
  cap_s6,
  NIG_ALPHA_S6,
  NIG_BETA_S6,
  current_log_ratio_s6
)
```

The final NAV is:

```text
nav_s6 = min(max(fair_nav_s6, intrinsic_s6), cap_s6 - deductible_s6)
```

The lending value is:

```text
lending_value_s6 = intrinsic_s6 * 800_000 / 1_000_000
```

This means:

- In-the-money cover can be borrowed against.
- Out-of-the-money cover can show positive NAV but zero lending value.
- At expiry, NAV equals intrinsic settlement value.

## Transaction Model

| Phase | Transactions | Notes |
|---|---:|---|
| Issue IL protection | One `accept_quote` tx after quote preview | Premium paid and terms stored |
| Midlife lending preview | One `preview_lending_value` tx or simulation | Reads live oracles and computes shifted NIG value |
| Settlement | One settlement tx | Uses settlement math against expiry prices |

The preview is read-only from an economic perspective. It returns the collateral mark but does not move USDC or settle the policy.

## Validation Data

Host unit tests in `crates/halcyon_il_quote/src/midlife.rs` cover:

| Test | Coverage |
|---|---|
| `unchanged_pool_has_positive_fair_nav_but_zero_lending_value` | OTM policy can have fair NAV while lending value stays zero |
| `price_move_creates_lendable_intrinsic_value` | Large price move produces intrinsic value and conservative advance |
| `expiry_nav_equals_intrinsic_value` | Remaining-days zero collapses NAV to settlement value |
| `no_lending_against_future_optionality_on_backtest_grid` | Price grid never lends beyond intrinsic advance policy |

Integration test `tests/integration/sol_il_midlife.spec.ts` covers:

1. Preview and accept an IL protection quote.
2. Use a live shifted crash oracle for SOL.
3. Simulate `preview_lending_value`.
4. Assert positive NAV.
5. Assert positive intrinsic payout.
6. Assert NAV payout is at least intrinsic payout.
7. Assert lending payout is no greater than 80% of intrinsic payout.
8. Assert CU stays below 1.4M.

## What This Proves

The current result proves:

- A live IL protection receipt can be valued on-chain after issuance.
- The midlife NAV uses the same NIG family as issuance, shifted by the live price move.
- The collateral mark is intentionally more conservative than NAV.
- No keeper-uploaded matrix or off-chain dealer mark is needed for IL midlife pricing.
- Expiry behavior is coherent: NAV equals intrinsic value when no time remains.

## What It Does Not Prove

The current result does not prove:

- A dense CU sweep across all price moves, sigmas, and remaining-day states.
- That lending protocols should advance against future option value. The current implementation explicitly does not.
- That 80% is the final liquidation advance rate. It is a product-risk parameter.
- That future concentrated-liquidity LP variants can reuse this exact formula. This report covers the synthetic 50/50 CPMM path.

## Operational Recommendations

1. Keep lending value intrinsic-only until there is a liquidation backstop specifically capitalised for future optionality.
2. Display NAV and lending value separately in the UI. They are economically different numbers.
3. Add a dense parity grid over remaining days, price ratios, sigma, deductible, and cap.
4. Preserve the USDC/USD oracle input even though it is near one; it is part of the pair-ratio definition and avoids hidden stablecoin assumptions.
5. Keep the engine version in the preview output so reports and UI can prove which pricing implementation was used.

## Source Map

| Area | Path |
|---|---|
| Product economics | `product_economics/il_protection_product_economics_report.md` |
| Math stack | `product_economics/il_protection_math_stack.md` |
| Host midlife pricer | `crates/halcyon_il_quote/src/midlife.rs` |
| Shifted NIG engine | `crates/halcyon_il_quote/src/insurance/european_nig.rs` |
| Settlement engine | `crates/halcyon_il_quote/src/insurance/settlement.rs` |
| On-chain preview wrapper | `programs/halcyon_il_protection/src/instructions/preview_lending_value.rs` |
| Product state | `programs/halcyon_il_protection/src/state.rs` |
| On-chain integration harness | `tests/integration/sol_il_midlife.spec.ts` |
