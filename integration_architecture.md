# Halcyon — Integration Architecture

**Version:** v1.0
**Purpose:** How the built numerical components of Halcyon compose with the Solana-program, keeper, and frontend layers that still need to be built. Not a greenfield specification; the blueprint for turning existing research-grade work into a deployed protocol.
**Audience:** You (the builder) and Claude Code (the implementation assistant). Specific enough that Claude Code can generate correct Anchor code from it; clear enough that you can catch mismatches between the pitch and the implementation.
**Scope:** Three products (flagship worst-of-3 equity autocall on SPY/QQQ/IWM, IL Protection, SOL Autocall), one shared capital kernel, supporting keepers, one frontend.

---

## Part 1 — Inventory and Target Shape

### 1.1 What exists today

**SolMath** — published to crates.io. The numerical primitives layer. Fixed-point transcendentals (`ln_fixed_i`, `exp_fixed_i`, `sqrt6`, `sincos6`), complex arithmetic for NIG characteristic functions, modified Bessel K₁ via Abramowitz & Stegun, NIG CDF recovery via COS, Black-Scholes with all five Greeks, European barrier options across all four barrier types, bivariate normal CDF, three-stage implied-volatility solver, weighted-pool math, supporting infrastructure (Heston, SABR helpers), Gauss-Legendre quadrature, and `triangle_probability_with_grad` (in `solmath-core/src/triangle_prob.rs`; consumed by the flagship gradient path, validated bit-exact via Stein's lemma against finite-difference). Validated against QuantLib, mpmath at 50-digit precision, and scipy across 2.5 million test vectors; median agreement with QuantLib on the high-precision path is fourteen decimal places. Feature-flagged so consumers pull only what they need. Requires no further work to ship.

**`halcyon-quote`** — single workspace crate housing Halcyon-specific pricing and business logic. Depends on SolMath via feature flags (transcendental, complex, pool, barrier, insurance, bs). Current contents:

*SOL Autocall pricers.*
- `autocall_v2.rs` (3,815 lines). Dense 64-node FFT-convolution pricer with sparse COS KI correction. Constants: `GRID_N = 64`, `N_OBS = 8`, `KNOCK_IN_LOG_6 = -356_675` (ln 0.70), `AUTOCALL_LOG_6 = 24_693` (ln 1.025), `NIG_ALPHA_1D = 13,040,000`, `NIG_BETA_1D = 1,520,000`. Exports `solve_fair_coupon`, `solve_fair_coupon_sol`, `solve_fair_coupon_at_vol`, `solve_fair_coupon_markov`, `AutocallParams`, `NigParams6`, `AutocallPriceResult`, `PriceConfidence`, `GatedPriceResult`.
- `autocall_v2_parity.rs` (503 lines). Gated Richardson CTMC, `PARITY_N1 = 10`, `PARITY_N2 = 15`, 10% convergence gap threshold, flags low-confidence quotes.
- `autocall_v2_e11.rs` (569 lines). POD-DEIM live-operator reduced-order pricer. `E11_LIVE_QUOTE_N_STATES = 50`, `E11_LIVE_QUOTE_D = 15`, `E11_LIVE_QUOTE_M = 12`, σ band [50%, 250%]. Gated by `live_quote_uses_e11(sigma, contract)`.

*SOL Autocall simulation and hedging.*
- `autocall_hedged.rs` (1,609 lines). Backtest harness with full hedge replay. Exports `HedgedAutocallError`, `VisibleState`, `HedgeMode`.
- `hedge_controller.rs` (1,248 lines). Nine target policies (`RawDelta`, `StateCap`, `KiTaper`, `PostKiZero`, `RecoveryOnly`, `CostAware`, `CallZoneOnly`, `DownsideLadder`, `ZoneAware`). Production policy is `delta_obs_050`: 50% initial hedge, rebalance on observation dates, 10% band, 75% delta cap, 1% minimum trade. Swap cost modelling built in.

*Flagship equity worst-of-3 pricer.*
- `worst_of_c1_filter.rs`. Projected c1 filter. Two live classes (safe, knocked). Retained nodes on the cumulative common-factor axis with attached (u, v) spread state. `MAX_K = 15` (the ceiling); production uses K=12 (`FROZEN_TABLES_K12`). Exports `FilterNode`, `FilterState`, `C1FilterTrace`, `QuoteWithDelta`, benchmark state types including `MaturityBenchState` (1,288 bytes). This is the on-chain worst-of pricer.
- `worst_of_c1_filter_gradients.rs`. Analytical gradient path through the filter's frozen-moment machinery. Exports `FrozenMomentTables<const K>` containing probability, `correction_u`, `correction_v` tensors keyed by observation × retained-node × region (safe_autocall, safe_ki, knocked_autocall), plus mean_u-interpolation tables at `MU_SAMPLES = [-2000, 0, 2000, 4000]` S6 units around `REFERENCE_SIGMA_COMMON_S6 = 364353`. Consumes `triangle_probability_with_grad` from SolMath (`solmath-core/src/triangle_prob.rs`), which is validated bit-exact against finite-difference via Stein's lemma.
- `k12_correction.rs`. Compile-time correction table addressing K=12 vs K=15 discretisation error. 256 entries of σ ∈ [0.08, 0.80] in S6 units, 2,048-byte storage as i64 micro-bps (256 × 8 bytes), Catmull-Rom interpolation at runtime (~150 CU per lookup). Applied additively to the K=12 filter's `fair_coupon_bps` output. The source file comment and prior documentation incorrectly state 512 bytes; the correct size is 2,048 bytes. i64 is required because values range beyond i32 (e.g. −1,283,678,286 micro-bps); i32 packing is not viable without reducing the value range, which is a separate engineering decision not taken at v1.

*IL Protection pricer.*
- `insurance/*` subtree. `nig_european_il_premium` is the production engine: 5-point Gauss-Legendre quadrature across four payoff regions plus tails, NIG density computed analytically via Bessel K₁, i128 accumulator at effective SCALE_18 to preserve low-sigma premiums. Validated across 2,027 backtested 30-day windows with zero engine failures. Shield variant (path-dependent) is out of scope for v1.

*Shared support.*
- EWMA volatility state machines, two-pass linear fair-coupon solvers, delta-surface builders, swap-cost models, fixed-point conversion helpers.

All three product pricers are deterministic fixed-point Rust, validated against reference implementations, and produce the backtested economics documented in the product reports: flagship ~8.4% buyer IRR / ~5.2–5.6% vault occupied-capital return / zero insolvencies over 20 years (one-factor NIG with per-quarter recalibration, 500 bps/obs fair-coupon ceiling); IL Protection 80% loss ratio / 3.0% vault annualised return across 2,027 windows; SOL Autocall 20.7% rolling-reinvest CAGR / 7-of-7 positive vault years.

**Calibration artefacts.** `output/factor_model/spy_qqq_iwm_factor_model.json` contains the flagship calibration: one `common_factor_nig: {alpha, beta, gamma}` object, one `common_factor_delta_scale_*` constant, one `common_factor_loadings` vector (SPY 0.516, QQQ 0.568, IWM 0.641 in the current calibration), one `residual_covariance_daily` matrix, per-asset martingale drifts. This structure proves the model form (see Part 1.3).

**Localnet scaffolds.** Test-only Anchor experiments, not production-shaped. Treated as throwaway for the integration build.

### 1.2 What does not exist today

No kernel program. No production product programs. No on-chain vault, reserve, tranche, policy-header, or fee-ledger state. No keeper processes beyond the backtest replay engines. No frontend. No TypeScript SDK. No ALTs. No deployed program at devnet or mainnet scale. No cross-program contract between a product and a kernel. No crate decomposition — `halcyon-quote` is one crate containing pricing logic for all three products; target state is one crate per product plus a small number of shared crates.

The daily-KI correction table (separate from the K=12 discretisation correction; see 1.4) is being built but not yet committed; the grid computation is in progress.

### 1.3 The flagship pricer: one-factor NIG with Gaussian residuals

The flagship worst-of-3 pricer uses a one-factor NIG construction, not a Gaussian copula over per-underlying NIG marginals. These are distinct model families and the architecture document must name the correct one or the protocol's risk model is misdescribed.

The model is:

```
X_i(t) = m_i · t + ℓ_i · F(t) + ε_i(t)
```

where:

- `F(t)` is a single univariate NIG process (the common factor) with parameters `(α_F, β_F, δ_F · t, 0)`. One NIG, shared across all three assets.
- `ℓ_i` is the per-asset scalar loading on the common factor. Current calibration: SPY 0.516, QQQ 0.568, IWM 0.641.
- `ε_i(t)` is a 3D correlated Brownian residual with covariance `Σ_ε · t`. Pure Gaussian, no heavy tails on the residual.
- `m_i` is the per-asset martingale drift enforcing zero-drift under the risk-neutral measure.

The joint characteristic function is:

```
φ(ξ; t) = exp(i · t · ⟨ξ, m⟩) · φ_F(⟨ξ, ℓ⟩; t) · exp(−½ · t · ξᵀ Σ_ε ξ)
```

Marginals of `X_i` are *not* NIG — they are convolutions of a scaled NIG with a Gaussian. Dependence arises from the shared factor, not from a copula. The calibration JSON's structure (one `common_factor_nig` object rather than three per-asset NIGs, one `common_factor_loadings` vector rather than a correlation matrix) confirms this directly. The validation MC simulates the factor form (IG subordinator for F, per-asset loading, additive Gaussian residual via Cholesky of Σ_ε); the 3D COS method's backward recursion uses the factor CF.

This structure is *required* for the on-chain path to be feasible. A generic Gaussian copula over NIG marginals has no closed-form joint characteristic function, which means no 3D COS spectral pricer. The one-factor construction gives heavy-tailed, asymmetric joint dynamics with a tractable joint CF — which the c1 filter and the cos3d validation depend on.

**Documentation hygiene note.** An earlier revision of `worst_of_autocall_product_economics_report.md` described the pricer as "NIG Gaussian copula MC" with per-asset NIG parameters and a Spearman-converted correlation matrix. This has been corrected to "one-factor NIG with Gaussian residuals" with factor loadings (SPY 0.516, QQQ 0.568, IWM 0.641) and Gaussian residual covariance. The architecture doc remains the canonical source for the model form.

### 1.4 Two distinct correction tables for the flagship

The flagship's on-chain quote path applies two additive correction tables on top of the K=12 c1 filter's output. Readers regularly conflate these, so they are named and scoped separately here:

**K=12 discretisation correction.** Compile-time lookup table in `k12_correction.rs`. Source: `correction[i] = fc_live_K15 − fc_frozen_K12_with_bearish_complement` sampled at 256 σ points. Addresses the error introduced by using the K=12 projected filter instead of a K=15 reference. Catmull-Rom interpolation on σ, ~150 CU at runtime, 2,048 bytes of binary data compiled into the flagship's quote crate (256 × 8 bytes; i64 required by the value range). Applied additively in bps to the K=12 filter's `fair_coupon_bps` output.

**Daily-KI cadence correction.** Separate compile-time lookup, built from a 3D COS spectral method with re-derived Fang-Oosterlee recursion (corrected to include the cross-term the canonical version drops). Addresses the error introduced by evaluating the 80% knock-in barrier only at quarterly observation dates rather than continuously-daily. Validated against 500,000-path Monte Carlo on a 9-cell grid within 1e-3 absolute accuracy. Serializes to canonical JSON; hash committed on-chain as SHA-256 for verifiability-at-recomputation. σ-indexed. Applied additively in bps to the output of the c1 filter + K=12 correction.

Both tables are compile-time data in the flagship quote crate's binary. Regenerating either requires a program upgrade (source bins → new crate version → redeploy). This is by design: correction tables are tied to the specific filter geometry and calibration they were built against, and mixing correction versions with filter versions is a silent-miscalibration hazard.

### 1.5 What the whitepaper positions that the build needs to support

The whitepaper positions Halcyon as three products sharing a single underwriting vault, with uncorrelated failure modes, running entirely on-chain with deferred hedge execution. The flagship references SPY/QQQ/IWM via Pyth equity feeds and hedges through xStocks (SPYx, QQQx, with IWM projected into the SPY/QQQ pair via a rolling 252-day regression). The integration architecture must reach this endpoint. Scope-narrowing happens inside the build order (see Part 4 in a later document), not by removing products from the target design.

### 1.6 Design invariants

Three non-negotiables from which everything else follows:

**Kernel owns money; products own math.** The kernel holds USDC, xStocks, and SOL. It owns reserve accounting, tranche accounting, policy-header lifecycle, fee ledger, hedge-book state. Product programs price, enforce product-specific invariants, and CPI into the kernel for every capital operation. No product program moves money.

**Off-chain for what is expensive or requires data Solana doesn't have.** Keepers compute fvol, OLS regressions, MC or amortised-analytical aggregate deltas, Jupiter routes. They write results to on-chain accounts with staleness gates. Keeper trust is explicit, bounded, and rotatable.

**One source of truth for each thing.** Pricers are not duplicated between backtest code and production code. Fee schedules are not duplicated between programs. Oracle decoding lives in one crate. This is the single property that most determines whether the protocol stays coherent as it grows; every other decision flows from wanting to preserve it.

### 1.7 What follows

Part 2 specifies the target system in detail — program topology, crate topology, state topology, lifecycle surface, hedge architecture per product, oracle layer, keeper inventory, upgrade posture, ALTs. Parts 3 through 6 (separate document sections, to be written next) cover the seams between existing and new components, the build order, and known tensions.

---

## Part 2 — Target System

### 2.1 Program topology

Four deployed programs:

- `halcyon_kernel` — money, reserves, tranches, policy-header lifecycle, hedge-book accounting, product registry, fee ledger, ALT registry, pause flags.
- `halcyon_flagship_autocall` — worst-of-3 equity autocall on SPY/QQQ/IWM.
- `halcyon_il_protection` — 30-day synthetic IL contract on SOL/USDC notional.
- `halcyon_sol_autocall` — 16-day SOL autocall with 2-day lockout.

No product program moves money. Every USDC transfer, every xStocks or SOL hedge-book mutation, every tranche accounting change goes through the kernel via CPI. Product programs price, validate product-specific terms, and call into the kernel for capital operations.

**Authentication.** Each product program has a fixed `product_authority` PDA. The kernel's `ProductRegistryEntry` (one per registered product) stores the expected authority. When a product CPIs into the kernel, it signs with its authority PDA via `invoke_signed`; the kernel checks the signer matches the registered authority. Only that product program can sign for its own PDA, so this gives the kernel a clean authentication primitive without knowing the product's internal dispatch.

### 2.2 Crate topology

```
halcyon/
  Cargo.toml                        workspace root
  programs/
    halcyon_kernel/
    halcyon_flagship_autocall/
    halcyon_il_protection/
    halcyon_sol_autocall/
  crates/
    halcyon_common/                 PDA seeds, fixed-point types, error codes, event schemas
    halcyon_kernel_types/           Kernel account layouts (for product-side cross-program reads)
    halcyon_oracles/                Pyth read helpers, staleness, price-ratio math
    halcyon_flagship_quote/         worst_of_c1_filter, worst_of_c1_filter_gradients,
                                    k12_correction, daily_ki_correction, factor-model types
    halcyon_il_quote/               insurance/*, EWMA helpers, IL-specific types
    halcyon_sol_autocall_quote/     autocall_v2, autocall_v2_parity, autocall_v2_e11,
                                    autocall_hedged, hedge_controller
    halcyon_client_sdk/             TypeScript IDL consumers, instruction builders (generated + hand)
  keepers/
    observation_keeper/
    regression_keeper/
    delta_keeper/
    hedge_keeper/
    regime_keeper/
  frontend/                         Next.js, wallet adapter, Anchor IDL consumption
  research/                         Existing backtest harnesses retained as-is
```

The decomposition from `halcyon-quote` (one crate) to three product-specific quote crates is a refactor, not a rewrite. Module boundaries in the existing code map cleanly:

| Existing file | Target crate |
|---|---|
| `autocall_v2.rs`, `autocall_v2_parity.rs`, `autocall_v2_e11.rs` | `halcyon_sol_autocall_quote` |
| `autocall_hedged.rs`, `hedge_controller.rs` | `halcyon_sol_autocall_quote` |
| `worst_of_c1_filter.rs`, `worst_of_c1_filter_gradients.rs` | `halcyon_flagship_quote` |
| `k12_correction.rs` | `halcyon_flagship_quote` |
| (new) `daily_ki_correction.rs` | `halcyon_flagship_quote` |
| `insurance/*` | `halcyon_il_quote` |
| (new) fixed-point helpers currently inlined | `halcyon_common` |

Each product crate has no Solana dependencies — no `solana_program`, no `anchor_lang`. It is the same crate consumed by the backtest harness in `research/`. This is the property that guarantees backtests and production share one pricing path.

**Why three crates, not one.** Each product program links only what it uses. SOL Autocall does not need the worst-of filter or IL quadrature in its BPF binary. Each product's compiled footprint stays auditable independently. One product's pricing can evolve without recompiling the others.

### 2.3 State topology

**Kernel-owned PDAs** (one entry per account type; instances per policy noted where relevant):

- `ProtocolConfig` (singleton) — admin multisig pubkey, global pause flags (issuance_paused_global, settlement_paused_global), per-product band widths and staleness caps, tranche split parameters, event thresholds, fee parameters, lookup-table authority, circuit-breaker flags, daily-KI correction SHA-256 commitment per product.
- `ProductRegistryEntry` (one per product program) — product program ID, expected `product_authority` PDA, product-specific coarse risk caps (per-pool and global), active/paused flag, engine version string, `init_terms` entry point selector.
- `VaultState` (singleton) — aggregate senior deposits, junior deposits, total reserved liability across all live policies, lifetime premium received, last-update slot/timestamp, utilization cap (90%).
- `SeniorDeposit` (one per senior depositor) — balance, last-deposit timestamp (for 7-day cooldown), accrued yield marker.
- `JuniorTranche` (one per junior depositor; founder-seeded at v1) — balance, non-withdrawable flag (true while any policy is active).
- `PolicyHeader` (one per live policy across all products) — product program ID, policy owner pubkey, notional, premium paid, max liability reserved, issue/expiry/settlement timestamps, terms hash, engine version, status enum (`Quoted` / `Active` / `Observed` / `AutoCalled` / `KnockedIn` / `Settled` / `Expired` / `Cancelled`), pointer to `ProductTerms` address, shard ID (v1 always 0; future sharding path).
- `CouponVault` (one per autocall product type) — separate pool funding coupon payments, independent of the underwriting reserve. Flagship and SOL Autocall each have their own; IL Protection has none.
- `HedgeSleeve` (one per hedged product) — separate capital sleeve holding USDC earmarked for hedge execution, distinct from underwriting reserve so hedge losses don't directly eat coupon funding. Holds token accounts for the hedge book's on-chain positions (SPYx + QQQx for flagship; SOL for SOL Autocall; not created for IL Protection).
- `HedgeBookState` (one per hedged product) — current token holdings per leg, target holdings from last rebalance, last-rebalance slot/timestamp, cumulative execution costs, last recorded aggregate delta for the event trigger.
- `AggregateDelta` (flagship only) — 3D aggregate delta vector (Δ_SPY, Δ_QQQ, Δ_IWM), Merkle root over `(note_pubkey, note_delta_at_sum)` pairs, Pyth spot snapshot at compute time, last-update slot/timestamp, live note count, delta-keeper-authority signature. SOL Autocall does not need this — its hedge keeper computes per-note delta on demand via the Richardson pricer.
- `Regression` (flagship only) — IWM regression coefficients: `β_SPY`, `β_QQQ`, `α`, `r²`, `residual_vol`, `window_start_ts`, `window_end_ts`, `last_update_slot`, `last_update_ts`, `sample_count`. 5-day staleness cap for writes to be accepted by downstream instructions.
- `VaultSigma` (one per product) — EWMA state: `ewma_var_daily` (45-day span, updated from Pyth log returns), `ewma_last_ln_ratio`, `ewma_last_timestamp`. Updated by a permissionless `update_ewma` instruction with 30-second rate limit.
- `RegimeSignal` (one per product that composes pricing sigma from off-chain regime state — currently IL Protection and SOL Autocall) — current fvol value, current regime enum (calm/stress), sigma multiplier (1.30 calm / 2.00 stress), sigma floor (40% annualised), last update timestamp. Written by the regime keeper, not permissionless (fvol requires off-chain historical computation).
- `FeeLedger` (singleton) — accumulated treasury fees awaiting sweep, per-product breakdown.
- `KeeperRegistry` (singleton) — authorized pubkeys per role: `observation`, `regression`, `delta` (flagship-only), `hedge` (per hedged product), `regime`. Admin-rotatable.
- `LookupTableRegistry` (one per product) — registered ALT addresses. Admin-mutable.

**Product-owned PDAs.**

`ProductTerms` — one per live policy, product-specific layout:

- **Flagship.** Entry prices for SPY, QQQ, IWM (from Pyth at issuance), barrier levels (autocall 100%, coupon 100%, KI 80%), full observation schedule (18 monthly coupon observations, 6 quarterly autocall observations, daily KI monitoring), memory coupon state (accumulated missed coupons), running accumulated coupons paid, autocall/KI status flags, filter-state bookkeeping for the c1 filter (retained nodes, common-factor path, spread state per observation), correction-table version pointers (K=12 and daily-KI hashes the policy was issued under).
- **IL Protection.** Pool descriptor (SOL/USDC Raydium CPMM, weight 50/50, deductible 1%, cap 7%), entry SOL price and entry USDC price from Pyth at issuance, notional, expiry timestamp, settlement status, regime snapshot at issuance (sigma used). No LP-token escrow — the product is synthetic, settled against Pyth entry/exit ratio.
- **SOL Autocall.** Entry SOL price, barrier levels (autocall 102.5%, coupon 100%, KI 70%), observation schedule (8 × 2-day), `no_autocall_first_n_obs = 1` (lockout), current observation index, autocall-lockout flag, quote share multiplier (75% of model fair coupon), issuer margin (50 bps). Coupon haircut state for accumulated coupons.

`ReducedOperators` — SOL Autocall only. Product-owned PDA holding the keeper-fed POD-DEIM reduced operators for the current pricing sigma: source `VaultSigma` / `RegimeSignal` slots, POD-DEIM table hash, and the two `15×15` reduced propagation maps `P_red_v(σ)` / `P_red_u(σ)`. The basis `Φ`, DEIM rows, and reconstruction matrices remain compile-time constants in the quote crate; only the live `P_red(σ)` pair is uploaded.

**On the flagship's three observation schedules.** The flagship product has three distinct observation cadences that serve different purposes, layered as follows. The c1 filter operates on `N_OBS = 6` — the six quarterly autocall observations. This is the only schedule the filter's backward recursion runs on; the filter is what prices the autocall-at-a-quarterly-barrier structure. Monthly coupon observations (18 total) are tracked outside the filter in `ProductTerms` memory-coupon state: on each 21-trading-day boundary, the observation keeper records whether the worst performer is above 100%, accumulating missed coupons or paying out stored ones. Daily KI monitoring (one check per trading day close) is handled via the daily-KI correction table, which converts the filter's discrete-quarterly KI probability into the equivalent daily-continuous-monitoring probability at pricing time; live KI breach events are recorded in `ProductTerms` via `record_ki_event`. So the filter prices 6 quarterly observations; the product layer tracks 18 monthly coupons and daily KI on top.

There is no on-chain `QuoteReceipt`. Quotes are computed by the frontend via the read-only `preview_quote` instruction; the buy transaction (`accept_quote`) recomputes at execution slot against slippage bounds. This eliminates the on-chain quote-commitment surface and removes TTL cleanup logic.

### 2.4 Lifecycle surface

All three products expose the same three-instruction lifecycle. Product-internal implementations differ; the surface is uniform so that keepers, frontends, and SDKs can treat the three products as instances of a common interface.

**`preview_quote`** — read-only. Takes current oracle state and product-specific parameters; returns `{premium, max_liability, quote_slot}`. No state writes. Frontend calls this before rendering the buy panel; its output is what the user sees.

**`accept_quote`** — the user's buy transaction. Instruction arguments are the product-specific terms plus `max_premium` (buyer will not pay more than this) and `min_max_liability` (buyer will not accept less protection than this). On-chain, the product program:

1. Reads current oracle state (Pyth prices, staleness check)
2. Reads current config (VaultSigma, RegimeSignal, Regression for flagship)
3. Calls the product's pricer from the quote crate with current state
4. Recomputes premium and max liability from the pricer's output plus product-specific spread (quote share, margin, tranche splits)
5. Checks `premium ≤ max_premium` and `max_liability ≥ min_max_liability`; aborts with `SlippageExceeded` if either fails
6. CPIs into the kernel's `reserve_and_issue` with the final numbers
7. Writes the product-specific `ProductTerms` account via the kernel's `init_terms` callback (mutual-CPI pattern — see 2.10)

**`settle`** — keeper-triggered at expiry or on an autocall/KI event. Product computes the payout from `ProductTerms` and current oracle state, CPIs into `apply_settlement`. Kernel clamps payout to reserved max liability, transfers USDC to buyer, releases any unused reservation to vault free capital, marks the policy settled.

**Intermediate observation instructions.** Some products have instructions between issuance and final settlement that update product-specific state without moving money:

- Flagship: `record_coupon_observation` (monthly), `record_autocall_observation` (quarterly), `record_ki_event` (daily, fires only when a name closes < 80%).
- IL Protection: none. European-settled at expiry only.
- SOL Autocall: `record_observation` (every 2 days, 8 total). Handles autocall, coupon accrual, and KI check in a single instruction.

These observation instructions update `ProductTerms`, may trigger an early `settle` in the same transaction (autocall or KI), and emit events that the delta and hedge keepers subscribe to.

### 2.5 The flagship on-chain pricing path

Because the flagship pricer is the most complex, it is worth specifying the on-chain path step-by-step.

At `preview_quote` and at `accept_quote` the product computes (all arithmetic in i64/i128 fixed-point; no floating point on-chain):

```
1. sigma_pricing_s6 = compose_sigma(VaultSigma, RegimeSignal)   // EWMA + regime multiplier + floor, i64 at SCALE_6
2. frozen_fair_coupon_bps = quote_frozen_k12(
       entry_prices, sigma_pricing_s6, factor_model_constants, schedule)   // i64, bps
3. k12_correction_micro_bps = k12_correction_lookup(sigma_pricing_s6)       // i64, micro-bps
4. daily_ki_correction_micro_bps = daily_ki_correction_lookup(sigma_pricing_s6)   // i64, micro-bps
5. total_correction_micro_bps = k12_correction_micro_bps + daily_ki_correction_micro_bps
6. live_fair_coupon_micro_bps = (frozen_fair_coupon_bps * 1_000_000) + total_correction_micro_bps
7. live_fair_coupon_bps = live_fair_coupon_micro_bps / 1_000_000    // integer divide, preserves precision until the final reduction
8. premium = apply_quote_share_and_margin(live_fair_coupon_bps, notional)   // i64, bps
9. max_liability = compute_max_liability(notional, KI_barrier, worst_case_recovery)
```

Corrections stay in micro-bps right up until the final reduction to bps at step 7. Keeping the accumulated correction in micro-bps preserves precision through the summation (two corrections each with ~6 significant digits combining into a single reduction). No f64 anywhere in the path — SolMath is i64/i128 fixed-point throughout, and the on-chain implementation matches.

Both correction tables are compile-time data in the `halcyon_flagship_quote` crate's binary. The `k12_correction` is 2,048 bytes at build time; the daily-KI correction is larger but still fits well within program size limits. Regenerating either requires a program upgrade. Their hashes are committed in `ProtocolConfig` for external verification: an auditor with the calibration, the code, and the spec can regenerate each table and verify the hash matches the on-chain commitment.

The factor-model constants (`α_F`, `β_F`, `δ_F` reference scale, common-factor loadings vector, residual covariance matrix, per-asset drifts) are compile-time data in the quote crate, sourced from `output/factor_model/spy_qqq_iwm_factor_model.json` at build time. Recalibration requires a program upgrade.

**On-chain CU budget.** The 955K CU budget documented in `MEMORY.md` is the ship target for the worst-of path. The total cost of filter + K=12 correction lookup + daily-KI correction lookup lands within this envelope; precise CU breakdowns are benchmarked per-build and documented at deploy time. The envelope leaves headroom under Solana's 1,400K CU transaction limit for surrounding instruction overhead (Anchor account validation, CPI setup, event emission). Single transaction, no priority-fee dependency for normal operation.

### 2.6 Hedge architecture, per product

**Flagship — deferred hedge with event-augmented triggers.**

The delta keeper recomputes aggregate delta each cycle using the *analytical* gradient path through `worst_of_c1_filter_gradients.rs`, consuming `triangle_probability_with_grad` from SolMath. Not Monte Carlo. The `FrozenMomentTables<K>` provide per-observation correction derivatives analytically; the gradient is validated bit-exact against finite-difference via Stein's lemma. Per-note delta extraction is ~2 ms off-chain — the backward step does a Gaussian-sum projection plus a KI triangle-probability evaluation per observation per region (safe_autocall, safe_ki, knocked_autocall), all analytically, across K=12 retained nodes and 6 quarterly observations. 100 live notes cost ~0.2 s per cycle. This is cheap enough that the keeper can run at 15-30 second cadence during market hours, dual-triggered on Pyth price-change events.

Delta keeper outputs go to `AggregateDelta`: the 3D vector (Δ_SPY, Δ_QQQ, Δ_IWM), a Merkle root over `(note_pubkey, per_note_delta_vector)` pairs, the Pyth spot snapshot used for the computation, timestamps, and the live note count. The Merkle commitment makes aggregation auditable — any auditor can re-run the analytical gradient from per-note `ProductTerms` + Pyth prices and verify the claimed sum.

Aggregate delta staleness is dual-triggered: reject if `elapsed_time > 30 min` OR `max|spot(now) − spot(last_compute)|/spot > 0.5%`. Handles quiet periods and gaps uniformly.

The hedge keeper wakes on a 5-day cadence floor (4-day on-chain minimum gap between rebalances) OR on an aggregate-delta-change event: if `|Δ_aggregate(now) − Δ_aggregate(last_rebalance)| > event_threshold` where `event_threshold = 1.5 × band_width`, the keeper wakes immediately. On wake it reads `AggregateDelta` and `Regression`, composes the 2D hedge target via the IWM projection:

```
target_SPY = Δ_SPY + β_SPY · Δ_IWM
target_QQQ = Δ_QQQ + β_QQQ · Δ_IWM
```

Compares to current `HedgeBookState`. If out of band, executes a Jupiter swap off-chain as the hedge-keeper signer, then CPIs `record_hedge_trade` to update on-chain state. Band width default: 0.5% of notional per leg, in `ProtocolConfig`, governance-tunable post-launch.

No inline Jupiter CPI at v1. Execution is off-chain, state update is on-chain.

**SOL Autocall — per-note observation-rebalanced hedge with `delta_obs_050` policy.**

No delta keeper. No aggregate delta account. The hedge keeper computes per-note delta on demand using the Richardson pricer (`price_autocall_v2_parity`) and `hedge_controller.rs`. Richardson is cheap enough (~2 seconds for a value-and-delta surface at N2=15) that on-demand computation beats maintaining additional on-chain state.

Hedge rebalance cadence: on each observation date (every 2 days). Additional intraperiod checks allowed if `allow_intraperiod_checks = true` in the hedge controller config, subject to `max_rebalances_per_day` cap. Target is computed per-note from the Richardson delta surface, interpolated to current spot, clipped to 75% delta cap, subject to 10% hedge band and 1% minimum trade. Aggregate target across live notes is the sum.

Execution: Jupiter swap off-chain, `record_hedge_trade` on-chain, identical pattern to the flagship. `HedgeSleeve` holds spot SOL. USDC drawn from the hedge sleeve, not from the underwriting reserve.

**IL Protection — unhedged.**

No hedge book, no hedge keeper, no rebalancing. The vault takes directional IL risk directly and earns the 20% premium margin plus the ×1.10 underwriting load. `HedgeSleeve` and `HedgeBookState` accounts are not created for this product. The product's variance is absorbed by the junior tranche.

### 2.7 Oracle layer

All price reads go through `halcyon_oracles`. Pull-model Pyth everywhere; no Switchboard at v1 (hooks only, not wired).

- **Flagship.** Pyth SPY, QQQ, IWM equity feeds. Availability on mainnet verified pre-deploy (deploy-gate check, not an assumption). Staleness cap: 30 seconds for quotes, 60 seconds for settlement.
- **IL Protection.** Pyth SOL/USDC only. No Raydium pool state reads — IL payout is computed synthetically against the theoretical CPMM formula `IL(r) = 2√r / (1 + r) − 1` using Pyth entry and exit price ratios. The product does not require the buyer to hold any LP tokens; it is a pure insurance contract on a notional LP position.
- **SOL Autocall.** Pyth SOL/USDC. Staleness cap: 30 seconds for observations and settlement.

The oracles crate exposes a uniform interface: `read_pyth_price(feed_account, now) -> Result<PriceSnapshot>`. The `PriceSnapshot` includes price, conf interval, publish slot, and publish timestamp. Callers apply per-product staleness caps.

### 2.8 Keeper inventory

Five keeper processes at v1, each authorized by a rotatable pubkey in `KeeperRegistry`:

**Observation keeper.** Triggers `settle` and product-specific observation instructions at their scheduled times. Per-product handlers:

- Flagship: monthly coupon (at each 21-trading-day boundary from issuance), quarterly autocall (at each 63-trading-day boundary), daily KI check (at each trading-day close, fires only when KI triggers).
- IL Protection: terminal settlement at T+30 days. No intermediate observations.
- SOL Autocall: observation at each 2-day boundary (8 total).

One process with per-product dispatch handlers sharing scheduling infrastructure.

**Regime keeper.** Daily compute of fvol from historical price history (off-chain). Writes `RegimeSignal` account with the current regime (calm/stress), sigma multiplier (1.30 / 2.00), sigma floor (40% annualised). Runs after the Pyth feed's daily-close window. Keeper-authority-gated write; rejected if the previous regime signal is less than 18 hours old. For the demo SOL Autocall path, this same keeper authority also computes the fixed-product POD-DEIM reduced operators off-chain and uploads the current `P_red_v(σ)` / `P_red_u(σ)` pair to the product's `ReducedOperators` PDA. A dedicated reduced-operator keeper role is deferred to post-v1 hardening; the demo intentionally reuses the regime keeper role to avoid another operational key.

**Regression keeper.** Daily compute of the IWM-vs-SPY/QQQ OLS regression from 252 trading days of history (off-chain history provider: Polygon, Databento, or IEX — single dependency, documented). Sanity-checks the latest close against Pyth (abort if divergence > 1%). Writes `Regression` with `β_SPY`, `β_QQQ`, `α`, `r²`, `residual_vol`, window timestamps, sample count. Keeper-authority-gated write. 5-day staleness cap on the `Regression` account for downstream instructions.

**Delta keeper.** Flagship only. Amortized analytical-gradient delta recomputation per cycle using the filter's frozen-moment machinery. Subscribes to issuance, observation, autocall, KI, and settlement events; wakes on any, plus dual-trigger staleness. Target cadence during US market hours: 15-30 seconds. Writes `AggregateDelta` with Merkle commitment.

**Hedge keeper.** Reads `AggregateDelta` (flagship) or computes per-note delta on demand via the Richardson pricer (SOL Autocall), composes the target, executes Jupiter swap off-chain, calls `record_hedge_trade`. Separate instances per hedged product because cadences and policies differ.

**EWMA updates** are permissionless with a 30-second on-chain rate limit. No dedicated EWMA keeper.

### 2.9 Upgrade and governance posture

**Kernel** starts upgradeable. Intended freeze path: once vault accounting, settlement, and reservation logic are stable post-mainnet (likely 3-6 months of live operation), revoke the upgrade authority via the Solana upgradeable loader and the kernel becomes immutable. Depositors can verify the binary hash against the audited source. Any future kernel change requires a new program at a new address and explicit migration. Kernel upgrades until then require admin multisig sign-off.

**Product programs** stay upgradeable longer. Each has its own upgrade authority. Pricing updates, correction-table refreshes, observation-schedule tweaks ship without touching the kernel or the other products. Version numbers in `ProductTerms` and `PolicyHeader` let the kernel enforce that policies settle under the engine version they were issued against, even after product upgrades.

**Parameter governance** through `ProtocolConfig`. Band widths, staleness caps, tranche splits, event thresholds, pause flags, sigma multipliers, sigma floors are all governance-mutable without program upgrade. Admin multisig at v1; no DAO governance.

**Circuit breakers** in `ProtocolConfig`:
- `issuance_paused_per_product` (blocks `accept_quote` for one product)
- `issuance_paused_global` (blocks all `accept_quote`)
- `settlement_paused_global` (emergency only; blocks all `settle`)
- `hedging_paused_per_product` (blocks `record_hedge_trade`; position runs unhedged under admin discretion, intended for Jupiter outage scenarios only)

Pause flags are admin-mutable with no timelock. Unpause requires the same admin.

### 2.10 The issuance CPI pattern

The product's `accept_quote` makes **two sequential CPIs into the kernel** inside a single transaction, with the product writing its `ProductTerms` account locally in between. There is no kernel→product callback; the kernel never invokes product code.

1. product → `kernel::reserve_and_issue`. Kernel validates (registered product, not paused, `max_liability` within per-policy and global risk caps, free capital sufficient, sigma and regime signals fresh, premium slippage within caller's bound). Kernel mutates (transfers premium USDC, splits into senior / junior / treasury shares, increments `vault_state.total_reserved_liability` and `product_registry_entry.total_reserved`, creates `PolicyHeader` with `status = Quoted`, stores the product-supplied `terms_hash`, leaves `product_terms = Pubkey::default()`).
2. Product writes `ProductTerms` at the expected `[b"terms", policy_id]` PDA, under its own program's ownership. No CPI — the product owns this account.
3. product → `kernel::finalize_policy`. Kernel rehashes the bytes of `ProductTerms` (discriminator + payload) and compares against `PolicyHeader.terms_hash`; validates `ProductTerms.owner == product_program_id`; flips `PolicyHeader.status` to `Active` and records the `product_terms` address.

All three steps are in the same transaction. Any failure rolls the whole flow back atomically.

This layout keeps responsibility clean: kernel owns `PolicyHeader` layout and invariants; product owns `ProductTerms` layout. The kernel never deserializes `ProductTerms` with a product-specific schema — it only hashes the raw bytes and records the address — but the hash rehash at `finalize_policy` binds the product's on-disk terms to the `terms_hash` the product committed to at `reserve_and_issue` time. A product that tries to commit one hash and write different bytes fails `finalize_policy` and the entire transaction rolls back.

**Two kernel entries, not one.** `PolicyHeader` exists in a `Quoted` intermediate state between the two CPIs. Splitting the lifecycle into two kernel instructions — each re-borrowing `PolicyHeader` cleanly on entry — sidesteps a class of Anchor account-constraints gotchas that bite if a single handler tries to mutate an account, CPI out, and mutate the same account again. See `LEARNED.md` for the related Anchor 0.32.1 seed-constraint aliasing bug, which applies to any kernel-owned `Account<T>` passed across a product→kernel CPI.

**`ProductRegistryEntry.init_terms_discriminator` is metadata only.** An earlier draft of this section described a kernel→product callback via that discriminator. That draft is superseded by the pattern above; the field survives as optional provenance metadata but nothing on-chain invokes it.

**Test obligations at L1 exit (replaces the re-entrance test this section used to call out):**

- **Happy path.** After `accept_quote` returns, `PolicyHeader.status == Active`, `PolicyHeader.product_terms` points at the expected PDA, `PolicyHeader.terms_hash` matches `sha256(product_terms.account_data)`, and vault/product reservations are incremented by `max_liability`.
- **Atomicity.** If the product aborts between `reserve_and_issue` and `finalize_policy` (panic, early return, explicit failure), `PolicyHeader` does not persist and reservations are unchanged. Transaction atomicity handles this, but the test pins the invariant.
- **Terms-binding enforcement.** `finalize_policy` with a `product_terms` account whose bytes do not hash to `PolicyHeader.terms_hash` fails with `TermsHashMismatch` and the transaction rolls back.
- **Status-machine integrity.** `finalize_policy` on a header not in `Quoted` (Active / Settled / absent) fails with `PolicyNotQuoted`.

### 2.11 Address Lookup Tables

All user-facing issuance and settlement transactions use v0 transactions with ALTs. Legacy transactions' 32-account limit is pressured on the flagship's issuance path (product program, kernel program, six tranche/vault/fee accounts, three Pyth feeds, policy header, terms, hedge sleeve token accounts, keeper registry, protocol config — already 14+ before user accounts).

Per-product lookup table holds the high-frequency static accounts: product program, kernel program, `ProtocolConfig`, relevant Pyth feeds, vault token accounts, shared PDAs. Per-transaction variable accounts (buyer, buyer token account, specific policy PDAs) are passed inline.

Lookup tables are protocol state. `register_lookup_table` and `update_lookup_table` are kernel instructions, admin-gated via the `ALTRegistry` authority.

### 2.12 Shard readiness (v1: not used, future-proofed in schema)

v1 runs a single `VaultState` and a single set of tranche PDAs. Production-scale sharding is not implemented. However, `PolicyHeader` includes a `shard_id` field (implicitly 0 at v1), and `reserve_and_issue` and `apply_settlement` carry the shard ID through the flow. This means adding sharding at v2 does not require an ABI break — only new instructions (`rebalance_shards`, `register_shard`) and an update to the routing logic that assigns a shard ID at issuance.

### 2.13 Named tensions and explicit v1 choices

Five places where v1 makes a specific choice that may need revisiting:

**Coupon vault vs. underwriting reserve.** Separate kernel-owned pools per hedged product. Matches the economics reports' "separate capital sleeve for hedging... own pool, separate from the coupon payment pool." Trade-off: multi-product capital cannot cross-subsidize at the margin. Conservative v1 choice; revisit at scale.

**Delta keeper trust surface.** Aggregate delta is computed off-chain by a single rotatable keeper. Merkle root over per-note deltas makes aggregation auditable. The analytical gradient path (verified bit-exact via Stein) is cheap to re-run from per-note state + Pyth prices, so an auditor can verify the keeper's output independently. This is a stronger trust posture than MC-based delta keepers because the recomputation is cheap.

**Daily-KI correction table vs. on-chain recomputation.** The daily-KI correction is a compile-time lookup table, committed via SHA-256, regenerable deterministically from the factor-model calibration. Not recomputed on-chain. The alternative — on-chain spectral recomputation — would blow the CU budget. The commitment + regenerability + deterministic inputs is the verifiability path.

**xStocks issuer counterparty.** The flagship hedge sleeve holds SPYx and QQQx from Backed Finance. If Backed has an operational incident, the hedge is impaired. v1 accepts this at demo scale ($50-$500 positions). Mitigation at scale: multiple wrapper issuers per underlying, not within hackathon scope.

**Pyth equity feed availability on mainnet.** Flagship pricing and observation depend on Pyth SPY/QQQ/IWM feeds remaining published within staleness tolerance. Feed outages during a scheduled observation cause observation failure (conservative default) or require manual admin intervention. Verified pre-deploy, not assumed.

These are named so that when the question comes up in an accelerator interview or an audit, the answer is "we considered it, here's why v1 resolves it this way, here's what changes at scale." The document is the source of record.

### 2.14 What Part 2 does not specify

This section defines the target system. It does not specify:

- Per-field byte counts, padding, serialization format. Those belong in a struct-layout document written alongside Layer 1 of the build, when Anchor's derive macros determine the actual layouts.
- Per-instruction account ordering. Belongs in an ABI document written alongside each product program's implementation, derived from the Anchor `#[derive(Accounts)]` macros.
- Event schemas for keepers to subscribe to. Specified per-instruction as each observation path is built.
- Test strategy. Implicit in each layer's exit criteria in a build-order document.

These are downstream documents. Writing them now, against code that does not exist, produces specifications that drift from what gets built. Part 2 specifies the target; the build creates the binding details.

---

## Part 3 — The Seams

This part specifies how existing Rust code plugs into the new Anchor programs. Every seam described here is a pattern that Claude Code will need to implement repeatedly; getting them documented precisely is what makes the build mechanical rather than speculative.

The seams are organised by direction: how a product program calls its quote crate, how a product CPIs into the kernel, how the kernel calls back into the product, how keepers consume pricer outputs, how the frontend reads and writes. Each seam has an invariant set and a concrete code shape.

### 3.1 The product-handler-calls-quote-crate seam

Every product program imports its quote crate as a pure-Rust dependency:

```toml
# programs/halcyon_sol_autocall/Cargo.toml
[dependencies]
anchor-lang = "..."
halcyon_sol_autocall_quote = { path = "../../crates/halcyon_sol_autocall_quote" }
halcyon_common = { path = "../../crates/halcyon_common" }
halcyon_oracles = { path = "../../crates/halcyon_oracles" }
halcyon_kernel_types = { path = "../../crates/halcyon_kernel_types" }
```

The quote crate has zero Solana dependencies. No `solana_program`. No `anchor_lang`. No `#[account]` derives. This is the invariant that keeps the pricing code usable by both the backtest harness and the on-chain program.

The product handler's pattern for any pricing-driven instruction is uniform:

```rust
// programs/halcyon_sol_autocall/src/instructions/preview_quote.rs
use halcyon_sol_autocall_quote::{
    solve_fair_coupon_at_vol, AutocallParams, AutocallPriceResult,
};
use halcyon_oracles::pyth;
use halcyon_common::fixed_point::scale_6_from_u64;

#[derive(Accounts)]
pub struct PreviewQuote<'info> {
    pub protocol_config: Account<'info, ProtocolConfig>,
    pub vault_sigma: Account<'info, VaultSigma>,
    pub regime_signal: Account<'info, RegimeSignal>,
    /// CHECK: Pyth price account, validated by halcyon_oracles
    pub pyth_sol: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(
    ctx: Context<PreviewQuote>,
    notional_usdc: u64,
) -> Result<QuotePreview> {
    // 1. Freshness checks (these fail early if state is stale)
    let now = ctx.accounts.clock.unix_timestamp as u64;
    require!(
        now - ctx.accounts.vault_sigma.last_update_timestamp
            <= ctx.accounts.protocol_config.sigma_staleness_cap_sec,
        HalcyonError::SigmaStale
    );

    // 2. Read oracle
    let sol_snapshot = pyth::read_price(
        &ctx.accounts.pyth_sol,
        now,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_sec,
    )?;

    // 3. Compose pricing sigma (EWMA + regime multiplier + floor)
    let sigma_s6 = compose_pricing_sigma(
        &ctx.accounts.vault_sigma,
        &ctx.accounts.regime_signal,
        ctx.accounts.protocol_config.sigma_floor_annualised_s6,
    )?;

    // 4. Call the pure-Rust pricer
    let priced: AutocallPriceResult = solve_fair_coupon_at_vol(sigma_s6)?;

    // 5. Apply product-level margin and compute premium + max_liability
    let premium = compute_premium(
        priced.fair_coupon_bps,
        notional_usdc,
        ctx.accounts.protocol_config.sol_autocall_quote_share_bps,
        ctx.accounts.protocol_config.sol_autocall_issuer_margin_bps,
    )?;
    let max_liability = compute_max_liability(
        notional_usdc,
        SOL_AUTOCALL_KI_BARRIER_BPS,
        SOL_AUTOCALL_MAX_RECOVERY_BPS,
    )?;

    Ok(QuotePreview {
        premium,
        max_liability,
        quote_slot: now,
        engine_version: CURRENT_ENGINE_VERSION,
    })
}
```

The invariants at this seam:

**All fixed-point conversions happen at the boundary.** Anchor-level arguments (`u64` notional in USDC base units) are converted to the quote crate's native types (`i64` at SCALE_6) before any call into the pricer. The conversion functions live in `halcyon_common::fixed_point` and are the only place these conversions happen. A product handler must not do ad-hoc conversions.

**The product handler never deserializes kernel internals.** It reads `ProtocolConfig`, `VaultSigma`, `RegimeSignal` through Anchor account macros using types from `halcyon_kernel_types`. It does not reach into raw byte representations.

**Freshness checks are first.** A stale sigma or a stale regime signal blocks the instruction before the oracle read, before the pricer call. This prevents pricing off stale inputs even if the pricer itself would tolerate them.

**Error types bubble up cleanly.** The quote crate's errors (`AutocallV2Error`, `HedgedAutocallError`, etc.) implement `From<_> for HalcyonError` in `halcyon_common::errors`, so handlers can use `?` without manual mapping.

### 3.2 The product-CPIs-kernel seam

The product's `accept_quote` is the most complex CPI in the system. It crosses from product to kernel, the kernel mutates money and creates `PolicyHeader`, then the kernel CPIs back into the product to initialise `ProductTerms`. Getting this right is worth the detail.

Accounts context (product side):

```rust
#[derive(Accounts)]
#[instruction(notional_usdc: u64, max_premium: u64, min_max_liability: u64)]
pub struct AcceptQuote<'info> {
    #[account(mut)] pub buyer: Signer<'info>,
    #[account(mut, token::mint = usdc_mint)]
    pub buyer_usdc: Account<'info, TokenAccount>,

    // Kernel accounts (passed through for CPI)
    pub kernel_program: Program<'info, HalcyonKernel>,
    #[account(mut)] pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(mut)] pub vault_state: Account<'info, VaultState>,
    #[account(mut)] pub fee_ledger: Account<'info, FeeLedger>,
    #[account(mut)] pub senior_tranche: Account<'info, SeniorTrancheAggregate>,
    #[account(mut)] pub junior_tranche: Account<'info, JuniorTrancheAggregate>,
    #[account(mut, token::authority = vault_authority)]
    pub vault_usdc: Account<'info, TokenAccount>,
    /// CHECK: PDA owned by kernel
    pub vault_authority: AccountInfo<'info>,
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    // Policy accounts (init by kernel, populated by product via kernel callback)
    #[account(init, payer = buyer, space = PolicyHeader::SPACE,
              seeds = [b"policy", policy_id.as_ref()], bump)]
    pub policy_header: Account<'info, PolicyHeader>,
    #[account(init, payer = buyer, space = SolAutocallTerms::SPACE,
              seeds = [b"terms", policy_id.as_ref()], bump)]
    pub product_terms: Account<'info, SolAutocallTerms>,

    // Product authority PDA (signs the CPI back into kernel)
    /// CHECK: derived PDA, signs via invoke_signed
    #[account(seeds = [b"product_authority"], bump)]
    pub product_authority: AccountInfo<'info>,

    // Config accounts (read-only for recompute)
    pub vault_sigma: Account<'info, VaultSigma>,
    pub regime_signal: Account<'info, RegimeSignal>,
    /// CHECK: Pyth, validated by halcyon_oracles
    pub pyth_sol: AccountInfo<'info>,

    // Coupon vault (kernel-owned, SOL Autocall specific)
    #[account(mut)] pub sol_autocall_coupon_vault: Account<'info, CouponVault>,

    // Hedge sleeve (SOL Autocall is hedged, so sleeve is required)
    #[account(mut)] pub sol_autocall_hedge_sleeve: Account<'info, HedgeSleeve>,

    // System accounts
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub clock: Sysvar<'info, Clock>,
}
```

Handler pseudocode with the full CPI sequence:

```rust
pub fn handler(
    ctx: Context<AcceptQuote>,
    notional_usdc: u64,
    max_premium: u64,
    min_max_liability: u64,
) -> Result<()> {
    // 1. Recompute premium and max_liability at execution slot
    //    (same code path as preview_quote; factored into a helper)
    let recomputed = recompute_quote_inline(&ctx, notional_usdc)?;

    // 2. Slippage gate
    require!(recomputed.premium <= max_premium, HalcyonError::SlippageExceeded);
    require!(
        recomputed.max_liability >= min_max_liability,
        HalcyonError::SlippageExceeded
    );

    // 3. Build terms hash (binds the policy to the exact inputs used)
    let terms_hash = hash_product_terms(&recomputed, &ctx.accounts);

    // 4. CPI into kernel::reserve_and_issue
    let bump = ctx.bumps.product_authority;
    let seeds: &[&[u8]] = &[b"product_authority", &[bump]];
    let signer_seeds = &[seeds];

    halcyon_kernel::cpi::reserve_and_issue(
        CpiContext::new_with_signer(
            ctx.accounts.kernel_program.to_account_info(),
            halcyon_kernel::cpi::accounts::ReserveAndIssue {
                product_authority: ctx.accounts.product_authority.to_account_info(),
                buyer: ctx.accounts.buyer.to_account_info(),
                buyer_usdc: ctx.accounts.buyer_usdc.to_account_info(),
                vault_usdc: ctx.accounts.vault_usdc.to_account_info(),
                vault_authority: ctx.accounts.vault_authority.to_account_info(),
                protocol_config: ctx.accounts.protocol_config.to_account_info(),
                vault_state: ctx.accounts.vault_state.to_account_info(),
                fee_ledger: ctx.accounts.fee_ledger.to_account_info(),
                senior_tranche: ctx.accounts.senior_tranche.to_account_info(),
                junior_tranche: ctx.accounts.junior_tranche.to_account_info(),
                product_registry_entry: ctx.accounts.product_registry_entry.to_account_info(),
                policy_header: ctx.accounts.policy_header.to_account_info(),
                coupon_vault: ctx.accounts.sol_autocall_coupon_vault.to_account_info(),
                hedge_sleeve: ctx.accounts.sol_autocall_hedge_sleeve.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer_seeds,
        ),
        ReserveAndIssueArgs {
            notional: notional_usdc,
            premium: recomputed.premium,
            max_liability: recomputed.max_liability,
            terms_hash,
            engine_version: CURRENT_ENGINE_VERSION,
            expiry_ts: recomputed.expiry_ts,
            shard_id: 0,
        },
    )?;

    // 5. Kernel has now created PolicyHeader with status = Quoted.
    //    Write ProductTerms (this is the product-side half of the mutual-CPI pattern).
    //    The kernel will CPI back into finalize_policy in a follow-up instruction
    //    within the same transaction, which flips status to Active after verifying
    //    ProductTerms was populated at the expected address.
    ctx.accounts.product_terms.set_inner(SolAutocallTerms {
        policy_header: ctx.accounts.policy_header.key(),
        entry_price_s6: recomputed.sol_entry_price_s6,
        autocall_barrier_s6: recomputed.autocall_barrier_s6,
        coupon_barrier_s6: recomputed.coupon_barrier_s6,
        ki_barrier_s6: recomputed.ki_barrier_s6,
        observation_schedule: recomputed.observation_schedule,
        no_autocall_first_n_obs: 1,
        current_observation_index: 0,
        quote_share_bps: ctx.accounts.protocol_config.sol_autocall_quote_share_bps,
        issuer_margin_bps: ctx.accounts.protocol_config.sol_autocall_issuer_margin_bps,
        accumulated_coupon_usdc: 0,
        status: ProductStatus::Active,
    });

    // 6. CPI back into kernel to flip PolicyHeader status from Quoted -> Active
    halcyon_kernel::cpi::finalize_policy(
        CpiContext::new_with_signer(
            ctx.accounts.kernel_program.to_account_info(),
            halcyon_kernel::cpi::accounts::FinalizePolicy {
                product_authority: ctx.accounts.product_authority.to_account_info(),
                policy_header: ctx.accounts.policy_header.to_account_info(),
                product_terms_addr: ctx.accounts.product_terms.to_account_info(),
            },
            signer_seeds,
        ),
    )?;

    // 7. Emit event
    emit!(PolicyIssued {
        policy_id: ctx.accounts.policy_header.key(),
        product_program_id: crate::ID,
        buyer: ctx.accounts.buyer.key(),
        notional: notional_usdc,
        premium: recomputed.premium,
        max_liability: recomputed.max_liability,
        issued_at: ctx.accounts.clock.unix_timestamp,
        entry_price_s6: recomputed.sol_entry_price_s6,
        barrier_ki_s6: recomputed.ki_barrier_s6,
        observation_count: recomputed.observation_schedule.count(),
    });

    Ok(())
}
```

The invariants at this seam:

**Authentication via `product_authority` PDA.** The CPI is signed via `invoke_signed` with the product's authority PDA. The kernel's `reserve_and_issue` handler's first check is that the signer matches `ProductRegistryEntry.expected_authority`. A caller cannot impersonate the product without access to that PDA, which is derivable only from the product program's ID.

**Two CPIs per issuance, not one.** The design initially looked like one CPI from product to kernel. In practice it is `product → kernel::reserve_and_issue` (kernel does accounting, creates header), then `product` writes `ProductTerms` locally, then `product → kernel::finalize_policy` (kernel verifies `ProductTerms` is populated, flips status). Two kernel entries because `PolicyHeader` exists in a `Quoted` intermediate state between the two. Both happen in the same transaction; atomic on success or rollback.

**Terms hash binds the policy.** The hash covers every input to the pricer: entry prices, sigma, regime signal snapshot, product parameters, engine version. Stored in `PolicyHeader`. Settlement verification can recompute and confirm the terms haven't been tampered with.

**Event emission before return.** The `PolicyIssued` event is the keeper's cue to start observing, delta-computing, and hedging. Emitting it inside the same transaction as the state mutations keeps the keeper's view consistent.

### 3.3 The kernel-validates-CPI seam

The kernel's `reserve_and_issue` is the only entrypoint for a product to reserve capital. Its validation is the safety-critical path. Pseudocode:

```rust
pub fn reserve_and_issue_handler(
    ctx: Context<ReserveAndIssue>,
    args: ReserveAndIssueArgs,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp as u64;

    // --- 1. Authentication ---
    require!(
        ctx.accounts.product_authority.is_signer,
        HalcyonError::ProductAuthoritySignatureMissing
    );
    require_keys_eq!(
        ctx.accounts.product_authority.key(),
        ctx.accounts.product_registry_entry.expected_authority,
        HalcyonError::ProductAuthorityMismatch
    );
    require!(
        ctx.accounts.product_registry_entry.active,
        HalcyonError::ProductPaused
    );

    // --- 2. Global pause checks ---
    require!(
        !ctx.accounts.protocol_config.issuance_paused_global,
        HalcyonError::IssuancePausedGlobal
    );
    require!(
        !ctx.accounts.product_registry_entry.paused,
        HalcyonError::IssuancePausedPerProduct
    );

    // --- 3. Capacity checks ---
    let vault = &ctx.accounts.vault_state;
    let new_reserved = vault.total_reserved_liability
        .checked_add(args.max_liability)
        .ok_or(HalcyonError::Overflow)?;
    let total_capital = vault.senior_deposits
        .checked_add(vault.junior_deposits)
        .ok_or(HalcyonError::Overflow)?;
    let utilization_bps = (new_reserved as u128 * 10_000u128)
        .checked_div(total_capital as u128)
        .ok_or(HalcyonError::Overflow)? as u64;
    require!(
        utilization_bps <= ctx.accounts.protocol_config.utilization_cap_bps,
        HalcyonError::UtilizationCapExceeded
    );
    require!(
        args.max_liability <= ctx.accounts.product_registry_entry.per_policy_risk_cap,
        HalcyonError::RiskCapExceeded
    );

    // --- 4. Transfer premium from buyer to vault ---
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.buyer_usdc.to_account_info(),
                to: ctx.accounts.vault_usdc.to_account_info(),
                authority: ctx.accounts.buyer.to_account_info(),
            },
        ),
        args.premium,
    )?;

    // --- 5. Split premium: 90% senior, 3% junior, 7% treasury ---
    let senior_share = (args.premium as u128 * 9000u128 / 10_000u128) as u64;
    let junior_share = (args.premium as u128 * 300u128 / 10_000u128) as u64;
    let treasury_share = args.premium - senior_share - junior_share;

    ctx.accounts.senior_tranche.add_premium(senior_share)?;
    ctx.accounts.junior_tranche.add_premium(junior_share)?;
    ctx.accounts.fee_ledger.add_treasury(treasury_share)?;

    // --- 6. Move funds into coupon vault and hedge sleeve if applicable ---
    //    (Product-specific; SOL Autocall uses both, IL Protection uses neither)
    if let Some(coupon_split) = args.coupon_vault_split {
        ctx.accounts.coupon_vault.add(coupon_split)?;
    }
    if let Some(hedge_split) = args.hedge_sleeve_split {
        ctx.accounts.hedge_sleeve.add_usdc_reserve(hedge_split)?;
    }

    // --- 7. Update vault state ---
    ctx.accounts.vault_state.total_reserved_liability = new_reserved;
    ctx.accounts.vault_state.lifetime_premium_received = ctx.accounts.vault_state
        .lifetime_premium_received
        .checked_add(args.premium)
        .ok_or(HalcyonError::Overflow)?;
    ctx.accounts.vault_state.last_update_ts = now;

    // --- 8. Create PolicyHeader in Quoted state ---
    ctx.accounts.policy_header.set_inner(PolicyHeader {
        product_program_id: ctx.accounts.product_registry_entry.product_program_id,
        owner: ctx.accounts.buyer.key(),
        notional: args.notional,
        premium_paid: args.premium,
        max_liability: args.max_liability,
        issued_at: now,
        expiry_ts: args.expiry_ts,
        settled_at: 0,
        terms_hash: args.terms_hash,
        engine_version: args.engine_version,
        status: PolicyStatus::Quoted,
        product_terms: Pubkey::default(), // filled by finalize_policy
        shard_id: args.shard_id,
    });

    Ok(())
}
```

The kernel's `finalize_policy` is the second half:

```rust
pub fn finalize_policy_handler(
    ctx: Context<FinalizePolicy>,
) -> Result<()> {
    // --- 1. Authentication (same pattern) ---
    require!(ctx.accounts.product_authority.is_signer, ...);
    require_keys_eq!(
        ctx.accounts.product_authority.key(),
        ctx.accounts.product_registry_entry.expected_authority,
        ...
    );

    // --- 2. Verify PolicyHeader is in Quoted state ---
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Quoted,
        HalcyonError::PolicyNotQuoted
    );

    // --- 3. Verify product_program_id matches the calling product ---
    require_keys_eq!(
        ctx.accounts.policy_header.product_program_id,
        ctx.accounts.product_registry_entry.product_program_id,
        ...
    );

    // --- 4. Record ProductTerms address and activate ---
    ctx.accounts.policy_header.product_terms = ctx.accounts.product_terms_addr.key();
    ctx.accounts.policy_header.status = PolicyStatus::Active;

    Ok(())
}
```

The invariants at this seam:

**Every numeric operation uses `checked_add` / `checked_mul`.** No silent overflow. Overflow is a `HalcyonError::Overflow` return; never a panic.

**Premium split uses u128 intermediates.** `args.premium as u128 * 9000u128 / 10_000u128` prevents overflow on the intermediate product before the division.

**Validation order is stable.** Authentication → global pause → capacity → token transfer → state mutation → account creation. A failure at any earlier stage rolls back everything; a failure at later stages cannot leave state partially mutated because Anchor transactions are atomic.

**PolicyHeader intermediate state.** The `Quoted` status is a real state in the lifecycle machine, not a flag. The flow `Active → Settled` is separate from `Quoted → Active`. Observation and settlement instructions both require the header to be in `Active`.

### 3.4 The flagship on-chain pricer seam

This is the most involved seam because the flagship has two correction tables layered on top of the c1 filter. Handler pseudocode (fixed-point throughout):

```rust
use halcyon_flagship_quote::{
    worst_of_c1_filter::quote_frozen_k12,
    k12_correction::k12_correction_lookup,
    daily_ki_correction::daily_ki_correction_lookup,
    FactorModelConstants, FLAGSHIP_FACTOR_MODEL,  // compile-time constants
};

pub fn price_flagship_fair_coupon(
    sigma_s6: i64,
    entry_prices_s6: [i64; 3],  // SPY, QQQ, IWM
    schedule: &ObservationSchedule,
) -> Result<i64> {
    // --- 1. K=12 filter backward pass (the main numerical work) ---
    let frozen_fair_coupon_bps: i64 = quote_frozen_k12(
        entry_prices_s6,
        sigma_s6,
        &FLAGSHIP_FACTOR_MODEL,   // one_factor_nig_params + loadings + residual_cov
        schedule,
    )?;

    // --- 2. K=12 discretisation correction ---
    let k12_correction_micro_bps: i64 = k12_correction_lookup(sigma_s6);

    // --- 3. Daily-KI cadence correction ---
    let daily_ki_correction_micro_bps: i64 = daily_ki_correction_lookup(sigma_s6);

    // --- 4. Accumulate in micro-bps, reduce once to bps ---
    let total_correction_micro_bps = k12_correction_micro_bps
        .checked_add(daily_ki_correction_micro_bps)
        .ok_or(HalcyonError::Overflow)?;

    let live_fair_coupon_micro_bps = (frozen_fair_coupon_bps as i128)
        .checked_mul(1_000_000)
        .ok_or(HalcyonError::Overflow)?
        .checked_add(total_correction_micro_bps as i128)
        .ok_or(HalcyonError::Overflow)?;

    let live_fair_coupon_bps: i64 = (live_fair_coupon_micro_bps / 1_000_000) as i64;

    Ok(live_fair_coupon_bps)
}
```

Consumed by `preview_quote`, `accept_quote`, and the settlement path (which uses the same pricer to recompute value at settlement time for KI cases).

The invariants at this seam:

**Factor-model constants are compile-time data.** `FLAGSHIP_FACTOR_MODEL` is baked into the `halcyon_flagship_quote` crate at build time from `output/factor_model/spy_qqq_iwm_factor_model.json`. The build script reads the JSON and generates a Rust constant. Recalibration is a program upgrade, not a runtime config change.

**Both correction tables are compile-time data.** `k12_correction_lookup` and `daily_ki_correction_lookup` are pure functions over σ that consult tables compiled into the binary. No on-chain accounts involved.

**SHA-256 commitments are verifiable on-chain.** `ProtocolConfig` stores the expected SHA-256 hash of each correction table. A deployment-time sanity check confirms the compiled binary's table hashes match the committed values. This is the verifiability-at-recomputation path.

**No f64 anywhere.** Every intermediate is i64 or i128. The `checked_` arithmetic protects against overflow that a f64 path would silently absorb.

### 3.5 The hedge-controller-to-keeper seam

The `hedge_controller.rs` module is pure Rust consumed by the hedge keeper. The keeper uses the same code path the backtest uses — no duplication. TypeScript-level code:

```typescript
// keepers/hedge_keeper/src/sol_autocall.ts
import { price_autocall_v2_parity, compute_hedge_target, HedgeControllerConfig }
    from '@halcyon/sol_autocall_quote_wasm';  // wasm-bindgen compilation of the Rust crate

async function processNote(note: PolicyRecord, pythSol: Price, sigma: number) {
    // 1. Re-price via Richardson to get current delta surface
    const priced = price_autocall_v2_parity(
        note.productTerms,
        sigma,
        pythSol.value,
        /* n1 */ 10,
        /* n2 */ 15,
    );
    if (priced.confidence === PriceConfidence.Low) {
        // Low confidence means Richardson didn't converge within gap threshold.
        // Log and skip this cycle; try again when sigma stabilises.
        logger.warn({ note: note.id, sigma }, 'Richardson low confidence');
        return;
    }

    // 2. Interpolate delta to current spot
    const modelDelta = priced.deltaSurface.interp(pythSol.value);

    // 3. Apply hedge controller policy (delta_obs_050)
    const config = HedgeControllerConfig.delta_obs_050();
    const target = compute_hedge_target(
        config,
        modelDelta,
        /* time_since_inception */ now() - note.issuedAt,
        /* observation_status */ note.currentObservationState,
    );

    // 4. Compare against current on-chain hedge book
    const current = await readHedgeBookState(note);
    const trade = target - current.position;

    // 5. Gate: band, minimum trade
    if (Math.abs(trade) < config.minTradeDelta * note.notional) return;
    if (Math.abs(trade) < config.hedgeBand * note.notional) return;

    // 6. Check cooldown (on-chain enforced too, but fail-fast here)
    if (now() - current.lastRebalanceTs < 4 * DAY_SECONDS) return;

    // 7. Execute Jupiter swap off-chain
    const swap = await jupiter.swap({
        inputMint: trade > 0 ? USDC_MINT : SOL_MINT,
        outputMint: trade > 0 ? SOL_MINT : USDC_MINT,
        amount: Math.abs(trade),
        slippageBps: 50,
        user: hedgeKeeperAuthority,
    });

    // 8. Slippage sanity against Pyth reference
    const executedPrice = swap.outputAmount / swap.inputAmount;
    const slippageBps = Math.abs(executedPrice - pythSol.value) / pythSol.value * 10_000;
    if (slippageBps > HEDGE_MAX_SLIPPAGE_BPS) {
        logger.error({ note: note.id, slippageBps }, 'Slippage exceeded; not recording trade');
        return;
    }

    // 9. Record on-chain
    await rpc.sendTransaction(
        await buildRecordHedgeTradeTx(note, swap, hedgeKeeperAuthority),
        [hedgeKeeperAuthority],
    );
}
```

The Rust crate compiled to WASM (via `wasm-pack`) gives the keeper access to the exact same `hedge_controller` logic the backtest uses. The `@halcyon/sol_autocall_quote_wasm` package is a build artefact of the workspace, generated in CI.

The invariants at this seam:

**One source of truth for hedge policy.** The keeper's `compute_hedge_target` is the same function the backtest calls. A hedge policy change is a crate update, not a keeper rewrite.

**Keeper trust boundary is clear.** The keeper signs trades with the `hedge_keeper_authority` keypair (registered in `KeeperRegistry`). That keypair's authority extends only to `record_hedge_trade` and the kernel's `update_hedge_book_state` instructions. It cannot issue policies, move premium, or touch senior/junior tranches.

**Slippage sanity is pre-commit.** If Jupiter executes at a price materially different from Pyth, the keeper does not record the trade on-chain. The hedge book stays out of sync with the actual token holdings, which will trigger a reconciliation step on the next cycle. Better than blindly trusting the execution price.

**Cooldown enforcement is defensive.** The on-chain instruction also enforces the 4-day minimum gap, but the keeper checks first to avoid wasting compute on transactions that will revert.

### 3.6 The delta-keeper-to-aggregate-delta seam

For the flagship. Uses the analytical gradient path from SolMath + `worst_of_c1_filter_gradients.rs`:

```typescript
// keepers/delta_keeper/src/flagship.ts
import { computeNoteDeltaAnalytic } from '@halcyon/flagship_quote_wasm';

async function runCycle() {
    // 1. Read live book (via RPC, deserialize PolicyHeader + ProductTerms for each)
    const notes = await readActiveFlagshipNotes();

    // 2. Read current Pyth prices for SPY, QQQ, IWM
    const spot = await readPyth(['SPY', 'QQQ', 'IWM']);

    // 3. Read current regression coefficients
    const regression = await readRegression();

    // 4. Compute per-note delta via analytical gradient
    const perNoteDeltas: [PublicKey, [i64, i64, i64]][] = [];
    let aggregate: [i128, i128, i128] = [0n, 0n, 0n];

    for (const note of notes) {
        // Pure-Rust analytical gradient, compiled to WASM
        const delta = computeNoteDeltaAnalytic(
            note.productTerms,
            spot,
            regression,
            FLAGSHIP_FACTOR_MODEL,
        );
        perNoteDeltas.push([note.policyId, delta]);
        aggregate[0] += BigInt(delta[0]);
        aggregate[1] += BigInt(delta[1]);
        aggregate[2] += BigInt(delta[2]);
    }

    // 5. Build Merkle root over (pubkey, delta) pairs
    const merkleRoot = buildMerkleRoot(perNoteDeltas);

    // 6. Publish per-note deltas to a public location for auditor verification
    //    (e.g. IPFS, S3, keeper-hosted HTTP endpoint — documented in keeper ops)
    await publishPerNoteDeltas(perNoteDeltas, merkleRoot);

    // 7. CPI update on-chain AggregateDelta
    await rpc.sendTransaction(
        await buildUpdateAggregateDeltaTx({
            deltaSpy: aggregate[0],
            deltaQqq: aggregate[1],
            deltaIwm: aggregate[2],
            merkleRoot,
            spotSnapshot: spot,
            liveNoteCount: notes.length,
            computedAt: Date.now() / 1000,
        }, deltaKeeperAuthority),
        [deltaKeeperAuthority],
    );
}
```

The invariants at this seam:

**Aggregate recomputed from scratch, not incremental.** Every cycle iterates the full live book. Do not try to maintain an incremental aggregate — delta is spot-dependent and additive updates are wrong the moment spot moves.

**Per-note deltas are published out-of-band for auditability.** The on-chain account holds only the aggregate and the Merkle root. An auditor retrieves per-note deltas from the keeper's published endpoint and verifies the aggregate by summing. If the keeper stops publishing, the Merkle root is still verifiable against the on-chain commitment, but audit becomes harder. Published deltas are the accountability mechanism.

**Event-driven wake keeps the aggregate fresh.** The keeper subscribes to `PolicyIssued`, `ObservationRecorded`, `AutocallTriggered`, `KnockInTriggered`, `PolicySettled` from the flagship program. On any of these, the next cycle runs immediately instead of waiting for the regular 15-30s cadence.

**Staleness bounds decide whether the hedge keeper consumes the output.** If elapsed time or spot drift exceeds the configured thresholds in `AggregateDelta`, the hedge keeper rejects the read and waits for a refresh. The delta keeper's job is to keep the read fresh; the hedge keeper's job is to refuse stale reads.

### 3.7 The observation-keeper-to-settlement seam

For all three products, observation and settlement triggers are handled by one keeper with product-specific dispatch. Pseudocode:

```typescript
// keepers/observation_keeper/src/scheduler.ts
async function tickCycle() {
    const now = currentTimestamp();

    // Flagship
    for (const note of await readActiveFlagshipNotes()) {
        // Monthly coupon observation
        if (isDueForCouponObservation(note, now)) {
            await triggerCouponObservation(note);
        }
        // Quarterly autocall observation
        if (isDueForAutocallObservation(note, now)) {
            await triggerAutocallObservation(note);
        }
        // Daily KI check
        if (isDueForDailyKiCheck(note, now)) {
            const kiBreached = await checkDailyKi(note);
            if (kiBreached) {
                await triggerKiEvent(note);
            }
        }
        // Expiry settlement
        if (now >= note.expiryTs) {
            await triggerSettle(note);
        }
    }

    // IL Protection
    for (const note of await readActiveIlProtectionNotes()) {
        if (now >= note.expiryTs) {
            await triggerSettle(note);
        }
    }

    // SOL Autocall
    for (const note of await readActiveSolAutocallNotes()) {
        if (isDueForObservation(note, now)) {
            await triggerObservation(note);
        }
        if (now >= note.expiryTs) {
            await triggerSettle(note);
        }
    }
}
```

Each `trigger*` function:
1. Builds the relevant instruction (e.g. `record_coupon_observation` for flagship, `record_observation` for SOL Autocall, `settle` for any).
2. Reads any required oracle accounts.
3. Signs with the observation keeper authority.
4. Sends the transaction.

The observation instructions themselves are on the product side; they update `ProductTerms`, may trigger an early `settle` in the same transaction (autocall or KI), and emit events.

The invariants at this seam:

**Observation keeper is idempotent.** Calling `record_coupon_observation` twice for the same observation date is a no-op on the second call (product checks `current_observation_index`). This means retries on RPC failure are safe.

**The keeper's authority is read-mostly.** It cannot transfer funds. It cannot issue policies. It can only trigger product-side observations and settlements, and those follow their own on-chain validation. A compromised observation keeper cannot drain the vault.

**Clock source is the Solana `Clock` sysvar, not keeper wall clock.** The keeper decides when to submit transactions, but the on-chain handlers check `Clock::get()?.unix_timestamp` against `ProductTerms` schedule fields. If the keeper submits late, the handler still runs correctly; if the keeper submits early, the handler rejects.

### 3.8 The frontend-read-write seam

Frontend is Next.js + Anchor IDL + wallet adapter. Three interaction patterns:

**Read-only preview.** Frontend calls the product's `preview_quote` via `simulateTransaction` with no signer and no state mutation, reads the returned premium / max_liability / quote_slot, displays to user.

```typescript
// frontend/src/lib/quote.ts
async function previewSolAutocall(notional: number): Promise<QuotePreview> {
    const ix = await program.methods
        .previewQuote(new BN(notional * 1e6))
        .accounts({ /* read-only accounts */ })
        .instruction();
    const tx = new VersionedTransaction(...).compile();
    const sim = await connection.simulateTransaction(tx, { sigVerify: false });
    return decodePreview(sim.value.returnData);
}
```

**Buy with slippage tolerance.** User sees the preview premium. Frontend adds slippage tolerance (default 50 bps) and submits `accept_quote`.

```typescript
async function buySolAutocall(notional: number, preview: QuotePreview, slippageBps = 50) {
    const maxPremium = Math.floor(preview.premium * (1 + slippageBps / 10_000));
    const minMaxLiability = Math.floor(preview.maxLiability * (1 - slippageBps / 10_000));

    const ix = await program.methods
        .acceptQuote(new BN(notional * 1e6), new BN(maxPremium), new BN(minMaxLiability))
        .accounts({ /* all required accounts via ALT */ })
        .instruction();

    // Build v0 transaction with the product's ALT
    const lookupTable = await getLookupTable(SOL_AUTOCALL_ALT);
    const message = new TransactionMessage({
        payerKey: wallet.publicKey,
        recentBlockhash: (await connection.getLatestBlockhash()).blockhash,
        instructions: [ix],
    }).compileToV0Message([lookupTable]);
    const tx = new VersionedTransaction(message);

    await wallet.signAndSend(tx);
}
```

On `SlippageExceeded` revert, frontend re-fetches the preview and retries (up to a limit).

**Read user positions.** Frontend scans `PolicyHeader` accounts owned by the user via `getProgramAccounts` with a filter on the owner field, joins with the corresponding `ProductTerms` for each. Renders position list.

The invariants at this seam:

**ALT usage from day one.** Every user-facing instruction uses a v0 transaction with the product's ALT. No legacy transactions. This is what keeps the account count manageable for the flagship's larger issuance path.

**Slippage tolerance is user-controllable.** Default 50 bps, user can adjust. Too tight and the transaction reverts often; too loose and the user pays a bad price. The UI should show both the displayed premium and the "max you'll pay at current slippage" as distinct numbers.

**No bespoke indexer at v1.** Position reads go directly against `getProgramAccounts` with owner filters. This is slower than an indexer but simpler to operate. Indexer is post-v1 scope.

**IDL-driven type safety.** The Anchor IDL generates TypeScript types for every account and instruction. No hand-written account deserializers. The generated types are the contract between frontend and program — if the program changes, the IDL regenerates, TypeScript compilation surfaces the mismatch before runtime.

### 3.9 Seam summary

Eight seams, each with a small number of invariants. Claude Code will implement each of them repeatedly across the four programs; having them documented once means the implementations converge on the same pattern rather than diverging into eight different approaches.

| Seam | What flows | Invariant |
|---|---|---|
| 3.1 Product → quote crate | Fixed-point inputs, `Result<AutocallPriceResult>` or similar | Quote crate has zero Solana deps; conversions at boundary |
| 3.2 Product → kernel CPI | Premium transfer, policy creation, tranche accounting | Mutual-CPI pattern (reserve_and_issue + finalize_policy); signed by product_authority PDA |
| 3.3 Kernel validation | Authentication, capacity, pause, atomicity | checked_ arithmetic, stable validation order |
| 3.4 Flagship pricer | Filter + 2 correction tables | All compile-time constants; SHA-256 verifiable; i64/i128 only |
| 3.5 Hedge keeper | Richardson reprice → controller → Jupiter | One source of truth for policy; pre-commit slippage sanity |
| 3.6 Delta keeper | Analytical gradient → aggregate + Merkle | From-scratch recomputation; per-note deltas published off-chain for audit |
| 3.7 Observation keeper | Schedule-driven instruction dispatch | Idempotent; limited authority; clock sysvar for validation |
| 3.8 Frontend | Preview → slippage-bounded accept → position read | ALT from day one; IDL-driven types; no bespoke indexer |

---

*End of Parts 1, 2, and 3. Parts 4 (Build Order), 5 (Known Tensions and Mitigations), 6 (Integration Risk Register) to follow.*
