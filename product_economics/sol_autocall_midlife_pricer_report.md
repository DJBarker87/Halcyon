# SOL Autocall Midlife Pricer Report

**Product:** 16-day SOL autocall  
**Use case:** live on-chain NAV and lending-value preview for an active SOL note  
**Current status:** production preview path wired through keeper-uploaded, hash-verified Markov transition matrices  
**Generated:** 2026-04-24

This report documents the midlife pricer used after a SOL autocall has been issued. The key distinction from issuance pricing is that midlife pricing does not solve a fair coupon. It values the remaining liability of a live note from the stored terms, current SOL price, current observation state, and fresh pricing sigma.

## Executive Summary

The SOL autocall midlife pricer provides a live collateral mark for the short-tenor crypto-native autocall. It computes NAV on-chain and then applies the same conservative KI-capped lending convention used by the flagship collateral flow.

The current implementation:

- Reads live SOL/USD from Pyth inside `preview_lending_value`.
- Reads the active policy header and SOL autocall terms from chain.
- Uses the current vault sigma plus regime signal to derive `sigma_pricing_s6`.
- Requires the keeper-fed midlife matrix PDA to match the current sigma and source oracle slots.
- Recomputes and checks SHA-256 commitments over the matrix-construction inputs and uploaded matrix values before using the matrix.
- Prices the remaining note with a 9-state Markov surface and precomputed transition matrices.
- Returns NAV, lending value, KI cap, coupon PV, par-recovery probability, current price, sigma, and state counters.

Latest validation coverage:

| Check | Result |
|---|---:|
| Host midlife unit tests | passing |
| Production 9-state grid vs 21-state reference | max allowed error 35,000 s6 |
| On-chain integration issue + preview | passing |
| On-chain preview CU cap | below 1.4M in integration |
| Matrix commitment recomputation in integration | passing |
| Matrix account hash domains | versioned SHA-256 domains |

The SOL path is intentionally more compact than the flagship path. The product is 16 days, single-underlying, and has only 8 observations, so the live collateral preview can fit in one transaction once the keeper has written the current transition matrix artefact.

## What It Prices

The pricer values the remaining liability of an active SOL autocall note per $1 notional:

```text
nav = PV(remaining coupons) + PV(redemption)
```

The output is scale-6 fixed point:

```text
1_000_000 = 1.000000 = par
```

The on-chain output bundle is:

| Field | Meaning |
|---|---|
| `nav_s6` | Present value of the remaining payoff per $1 notional |
| `ki_level_usd_s6` | KI barrier as a ratio to entry |
| `lending_value_s6` | Conservative collateral value after haircut and KI cap |
| `nav_payout_usdc` | Notional-scaled NAV payout |
| `lending_value_payout_usdc` | Notional-scaled lendable value |
| `remaining_coupon_pv_s6` | PV of due plus future coupons |
| `par_recovery_probability_s6` | Redemption component clamped to `[0, par]` |
| `sigma_pricing_s6` | Annualised sigma used for the run |
| `current_price_s6` | Current SOL/USD oracle price |
| `current_observation_index` | Next observation index stored in the terms |
| `due_coupon_count` | Coupons due at already-reached observations not yet recorded |
| `future_observation_count` | Remaining observation count used in the schedule |
| `model_states` | Markov state count, currently 9 |

The lending value is:

```text
ki_level_s6      = ki_barrier / entry_price
nav_haircut      = nav_s6 - 100_000
ki_cap_haircut   = ki_level_s6 - 100_000
lending_value_s6 = max(0, min(nav_haircut, ki_cap_haircut))
```

For the current 70% KI barrier, a healthy note is capped near:

```text
70% - 10% = 60% of notional
```

This is deliberately conservative. The lender never advances against a value above the KI-protected liquidation level less haircut.

## Product Backtest Context

The product economics are documented in `product_economics/sol_autocall_product_economics_report.md`.

Current product structure:

| Item | Value |
|---|---|
| Underlying | SOL/USD |
| Tenor | 16 calendar days |
| Observations | 8, every 2 days |
| Autocall barrier | 102.5% of entry |
| Coupon barrier | 100% of entry |
| KI barrier | 70% of entry, observation-date discrete |
| Autocall lockout | First observation suppressed |
| Quote share | 75% of model fair coupon |
| Issuer margin | 50 bps per note |
| NIG alpha / beta | 13.04 / 1.52 |
| Vol input | EWMA-45 scaled to annual sigma |

Historical economics from the current SOL product report:

| Metric | Value |
|---|---:|
| Backtest window | August 2020 to March 2026 |
| Possible entry windows | 2,042 |
| Notes issued | 1,638 |
| Issuance fraction | 80.2% |
| Autocall rate after lockout | about 68% |
| KI rate after lockout | about 6% |
| Mean holding period | 9.3 days |
| Buyer mean return after lockout | +1.65% per note in the before/after comparison |
| Vault edge after lockout | $16.94 per $1,000 note in the before/after comparison |

The midlife pricer matters because the note becomes useful as collateral only if a lending protocol can compute a live value without trusting the issuer.

## How It Works On-Chain

The production preview instruction is:

```text
preview_lending_value(
  protocol_config,
  vault_sigma,
  regime_signal,
  policy_header,
  product_terms,
  midlife_matrices,
  pyth_sol
)
```

The instruction flow:

1. Requires the policy header and product terms to be active.
2. Checks vault sigma freshness.
3. Checks regime signal freshness.
4. Reads fresh SOL/USD from Pyth.
5. Composes pricing sigma from vault sigma, regime signal, protocol floor, and protocol ceiling.
6. Requires the midlife matrix PDA to match the pricing sigma and source oracle slots.
7. Recomputes the matrix SHA-256 commitments and rejects mismatches.
8. Builds matrix references from the row-major account data.
9. Calls `price_midlife_nav_with_matrices`.
10. Returns the `LendingValuePreview` struct.

The preview itself is one transaction. The keeper matrix update is a separate maintenance path and can be performed before borrowers arrive.

## Keeper Matrix Path

The midlife pricer uses precomputed 9 x 9 transition matrices:

```text
states = 9
matrix_len = 81
cos_terms = 13
```

The keeper builds upload payloads with:

```text
cargo run -p halcyon_sol_autocall_quote --bin sol_midlife_matrices -- <sigma_ann_s6> <step_days_s6>...
```

The on-chain writer is:

```text
write_midlife_matrices(begin_upload, step_index, step_days_s6, start, values)
```

The matrix account stores:

| Field | Purpose |
|---|---|
| `sigma_ann_s6` | Sigma the matrix was built for |
| `n_states` | Must equal 9 |
| `cos_terms` | Must match the compiled compact COS setting |
| `uploaded_step_count` | Number of uploaded step lengths |
| `uploaded_lens` | Per-step uploaded value count |
| `step_days_s6` | Step length in days at S6 |
| `source_vault_sigma_slot` | Vault sigma slot used for the upload |
| `source_regime_signal_slot` | Regime signal slot used for the upload |
| `construction_inputs_sha256` | Commitment to deterministic builder inputs |
| `matrix_values_sha256` | Commitment to row-major matrix values |
| `matrices` | Flattened i64 transition matrix values |

## Matrix Verification

The verification pattern matches the flagship daily-KI correction commitment pattern, but it is applied to the SOL midlife matrix PDA.

There are two domain-separated hashes:

```text
halcyon:sol-autocall:midlife-matrix-inputs:v1
halcyon:sol-autocall:midlife-matrix-values:v1
```

`construction_inputs_sha256` covers:

- Matrix account version.
- SOL autocall engine version.
- Pricing sigma.
- Matrix shape.
- Observation schedule constants.
- KI and autocall log barriers.
- NIG training alpha, beta, and reference step.
- Source vault-sigma and regime-signal slots.
- Uploaded step count and each uploaded step length.

`matrix_values_sha256` covers:

- The construction-input hash.
- Uploaded step count.
- Each step length and uploaded length.
- The flattened row-major matrix values as signed i64 little-endian.

An external verifier can:

1. Read the matrix PDA.
2. Recompute `construction_inputs_sha256`.
3. Regenerate the transition matrices deterministically from the committed inputs.
4. Compare regenerated row-major values to the PDA.
5. Recompute `matrix_values_sha256`.
6. Reject the matrix if either hash differs.

The on-chain preview does steps 2 and 5 against the stored account contents before it prices.

## Math

The live state is represented by the SOL ratio:

```text
r = current_price / entry_price
```

The payoff conditions are:

```text
coupon_hit(t_i)   = S(t_i) >= coupon_barrier
autocall_hit(t_i) = S(t_i) >= autocall_barrier and i >= no_autocall_first_n_obs
ki_hit(t_i)       = S(t_i) <= ki_barrier
```

The pricer first handles already-due observations using the current oracle price. This prevents the lending mark from ignoring a coupon or terminal event that is due but has not yet been recorded by the observation keeper.

If the note has already reached a terminal condition:

```text
future_nav = terminal_principal
```

Otherwise it builds the remaining schedule and solves two Markov surfaces:

| Surface | Meaning |
|---|---|
| `nav` | Redemption plus coupon value |
| `redemption` | Redemption-only value |

The model carries separate untouched and touched KI states. At each future observation:

1. Apply the transition matrix for the step length.
2. Move probability mass through the KI boundary.
3. Apply autocall absorption when allowed.
4. Add coupon value when the coupon barrier is met.
5. Continue untouched and touched values separately.

The current spot ratio is linearly interpolated across the 9-state grid. If the policy has already latched KI, interpolation uses the touched surface; otherwise it uses the untouched surface.

## Transaction Model

There are two separate phases:

| Phase | Transactions | Notes |
|---|---:|---|
| Keeper matrix upload | One or more chunked `write_midlife_matrices` txs | Maintenance path, done when sigma/regime source slots change |
| Borrow/lending preview | One `preview_lending_value` tx or simulation | Uses already-written matrix account |

In the current integration harness, a single 81-value step matrix is uploaded in two chunks of 48 and 33 values. Production can upload more step lengths if the live remaining schedule needs them.

## Validation Data

Host unit tests in `crates/halcyon_sol_autocall_quote/src/midlife.rs` cover:

| Test | Coverage |
|---|---|
| `healthy_note_lending_value_is_ki_capped` | Healthy NAV above par still lends only up to KI cap less haircut |
| `knocked_note_tracks_live_recovery` | Latched KI at terminal tracks current SOL recovery |
| `due_coupon_is_included_before_keeper_records_it` | Due observation coupon is counted before keeper state advances |
| `future_value_declines_after_coupon_miss_backtest` | Below-coupon live state has lower NAV than at-barrier state |
| `no_overstated_lending_value_across_price_backtest_grid` | Lending value never exceeds NAV or KI cap on a price grid |
| `production_state_grid_tracks_higher_state_reference_backtest` | 9-state production grid tracks 21-state reference within tolerance |

Integration test `tests/integration/sol_il_midlife.spec.ts` covers:

1. Upload SOL reduced operators for issuance.
2. Upload SOL midlife matrices.
3. Fetch the matrix PDA.
4. Recompute both matrix SHA-256 commitments client-side.
5. Issue a SOL autocall policy.
6. Simulate `preview_lending_value`.
7. Assert positive NAV, lending value no greater than NAV, expected sigma, model state count 9, and CU below 1.4M.

## What This Proves

The current result proves:

- A live SOL autocall note can be valued on-chain after issuance.
- The lending mark is not an issuer-supplied value.
- The keeper matrix is not blindly trusted; the account contents are bound to deterministic commitments.
- The one-transaction preview path stays below the Solana compute cap in the current integration state.
- The collateral value is conservative by construction because it is capped at KI level less haircut.

## What It Does Not Prove

The current result does not prove:

- A dense production CU sweep across every possible second-level remaining schedule.
- That every future matrix upload will include every needed fractional step length unless the keeper is run with the correct schedule inputs.
- That real lending protocols will accept the haircut without further liquidation discounts.
- That future Solana compute-cost changes preserve current headroom.

## Operational Recommendations

1. Keep matrix uploads as a required keeper duty whenever sigma or regime source slots change.
2. Recreate the devnet matrix PDA after account-layout changes; old matrix accounts cannot deserialize under the new hash fields.
3. Include the two SHA-256 commitments in the UI or block-explorer drilldown for judge verification.
4. Add a denser SOL midlife parity artifact that sweeps time-to-observation, spot ratio, sigma, and KI-latched state.
5. Treat the lending cap as product policy, not a pricing artifact. Relaxing it changes lender risk.

## Source Map

| Area | Path |
|---|---|
| Product economics | `product_economics/sol_autocall_product_economics_report.md` |
| Math stack | `product_economics/sol_autocall_math_stack.md` |
| Host midlife pricer | `crates/halcyon_sol_autocall_quote/src/midlife.rs` |
| Matrix upload binary | `crates/halcyon_sol_autocall_quote/src/bin/sol_midlife_matrices.rs` |
| On-chain preview wrapper | `programs/halcyon_sol_autocall/src/instructions/preview_lending_value.rs` |
| Matrix writer | `programs/halcyon_sol_autocall/src/instructions/write_midlife_matrices.rs` |
| Matrix account state | `programs/halcyon_sol_autocall/src/state.rs` |
| Commitment documentation | `docs/audit/sol_midlife_matrix_verification.md` |
| On-chain integration harness | `tests/integration/sol_il_midlife.spec.ts` |
