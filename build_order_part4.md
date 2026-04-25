# Halcyon — Integration Architecture, Part 4: Build Order

**Version:** v1.0
**Scope:** The sequence of work to reach a mainnet-ready v1 protocol with all three products, starting from the repository state described in Parts 1–3. Not a greenfield plan; an ordering of the integration work specified in Parts 2 and 3, with exit criteria for each stage.
**Audience:** The builder and the implementation assistant. Specific enough to drive commit-level decisions; loose enough that early layers don't prescribe decisions that properly belong to later ones.
**Assumed capacity:** One full-time builder, 8–16 weeks.

---

## 4.1 Shape of the plan

The target is a mainnet-deployed protocol running three products against a shared kernel, with keepers operating against live oracles and a frontend that a retail user can actually transact through. Reaching that endpoint from the current repository is not a linear march — several components can only be validated against each other, and several have external dependencies whose resolution takes weeks of calendar time (regulated counsel, Backed Finance operational relationship, xStocks wrapper liquidity). The build order is therefore structured to:

1. **Prove the seam patterns once, with the product that has the fewest external dependencies** — then replicate for the other two. Refactoring eight seams simultaneously across three products is how integration projects fail; refactoring them once and then applying a proven template is how they ship.
2. **Put external-dependency work on the critical path early**, so calendar-bound items (legal engagement, Pyth equity feed verification, Jupiter SPYx/QQQx route depth check) run in parallel with in-repo build work instead of blocking at the end. There is no Backed Finance onboarding on the critical path — per the integration architecture §2.13, v1 accepts SPYx and QQQx as ordinary SPL tokens routed through Jupiter at demo scale ($50–$500 positions), with no counterparty integration. Multi-issuer routing is post-v1.
3. **Defer the most complex pricer's on-chain seam until the kernel and keeper surfaces are mature** — the flagship is built last, not because it's least important, but because it is the highest-variance integration and should only be attempted against a stable base.

This yields six layers. Each has a single exit criterion that determines whether the next layer can start. Skipping an exit criterion to "get ahead" is the failure mode; in a four-month build the time saved by skipping ahead is always recovered later at higher cost.

**Layer summary.**

| Layer | Duration | What ships | Gate to next layer |
|---|---|---|---|
| L0 — Foundations | Week 1–2 | Workspace split, crate decomposition, CI, localnet, ALT plumbing | Quote crates compile with zero Solana deps; backtest harness still passes |
| L1 — Kernel | Week 2–4 | `halcyon_kernel` program with full lifecycle surface, mutual-CPI pattern verified | Localnet end-to-end policy issue → observe → settle with a stub product |
| L2 — SOL Autocall end-to-end | Week 4–7 | First product program, observation keeper, hedge keeper, CLI settlement replay | Devnet: issuance + full observation cycle + autocall + hedge keeper rebalance, all against live Pyth SOL |
| L3 — IL Protection | Week 7–9 | Second product program, regime keeper, settlement math against live Raydium pool | Devnet: 30-day issuance → settlement with real IL on a testnet pool |
| L4 — Flagship | Week 9–13 | Third product program, regression keeper, delta keeper, full Merkle-audited hedge path, Jupiter spot swaps of SPYx + QQQx | Devnet: flagship issuance with all three correction tables live, hedge keeper round-trip through Jupiter into SPYx/QQQx (mock pool if mainnet routes not accessible from devnet) |
| L5 — Frontend + Mainnet Readiness | Week 13–16 | Next.js app replacing `app/` demo, audit prep, mainnet config, multisig setup, monitoring | Mainnet deploy with one product active, others paused, full monitoring in place |

Each layer spans 2–4 weeks. The uneven distribution reflects integration risk, not volume of code: L1 is short because the kernel's surface is small and well-specified; L4 is long because the flagship carries three correction tables, a delta keeper with Merkle commitments, and the IWM projection hedge. Calendar items that don't map to layers (legal engagement, Pyth equity feed verification on mainnet, mainnet multisig ceremony) run in parallel and feed L5.

The rest of this part specifies each layer in detail: what code is written, what is *not* written, how the layer is validated, and the exit criterion that must hold before moving on.

---

## 4.2 Layer 0 — Foundations

**Goal.** Reshape the repository so the new code has somewhere coherent to land. End state: an Anchor-compatible workspace where `crates/` hold pure-Rust product math, `programs/` is empty but ready, `keepers/` is empty but ready, CI runs, and the existing backtest harness still passes against the refactored crates.

This layer is bookkeeping, not feature work. It is tempting to skip it and start writing the kernel program in the existing shape. The reason not to: the existing `halcyon-quote` crate bundles all three products' math in a single compilation unit, which means every product program will pull in every other product's pricer via transitive deps. That is a BPF binary-bloat problem (every product program carries dead code for the other two products; this inflates deploy costs and obscures audit scope), a CU-budget-auditing problem (you can't tell what the flagship spends CU on if it's linking in code it doesn't call), and a release-cadence problem (bumping the SOL Autocall pricer shouldn't require redeploying the flagship). The split has to happen before any on-chain code is written against these crates — benchmarking the actual BPF size and deploy cost of each program separately is a worthwhile L0 measurement but it's not the gating argument.

### 4.2.1 Work

**Workspace reshape.** Convert the current `crates/halcyon-quote` and `solmath-core` layout to the crate topology specified in 2.2:

```
crates/
  halcyon_common/                    NEW — PDA seeds, fixed-point types, error codes
  halcyon_kernel_types/              NEW — kernel account layouts, exported for read
  halcyon_oracles/                   NEW — Pyth read helpers
  halcyon_flagship_quote/            EXTRACTED from halcyon-quote
  halcyon_il_quote/                  EXTRACTED from halcyon-quote
  halcyon_sol_autocall_quote/        EXTRACTED from halcyon-quote
  halcyon_client_sdk/                NEW — stub, grows across layers
```

`solmath-core` stays as-is — it's published and any changes to it are a separate upstream release concern.

The extraction is a `mv` operation per file plus `Cargo.toml` rewiring, not a logic rewrite. The existing module boundaries (per the table in 2.2) are already clean enough to split without code changes. The one piece of real work: any helpers that are currently inlined across products (fixed-point scaling, PDA seed constants, error enums) get hoisted into `halcyon_common` and re-exported.

**`programs/` and `keepers/` scaffolding.** Create:

```
programs/
  halcyon_kernel/
  halcyon_flagship_autocall/
  halcyon_il_protection/
  halcyon_sol_autocall/
keepers/
  observation_keeper/
  regression_keeper/
  delta_keeper/
  hedge_keeper/
  regime_keeper/
```

Each has a `Cargo.toml` pointing at the right quote crate and `halcyon_common`, a `lib.rs` with the Anchor `declare_id!` macro and an empty `#[program]` block, and nothing else. No instructions, no handlers. These exist so the workspace resolves and `anchor build` succeeds against an empty protocol. Every subsequent layer adds handlers; this layer just reserves the filenames.

**CI.** A `.github/workflows/ci.yml` (or equivalent) that runs, on every commit:

- `cargo test --workspace` against the pure-Rust crates.
- `anchor build` for each program.
- `anchor test --skip-deploy --skip-build` where there's nothing to run yet but the harness exists.
- The existing backtest replay at `make sol-autocall` against a checked-in JSON sample.

The backtest passing against the refactored crates is the single most important verification in L0. If it passes, the extraction preserved correctness; if it fails, an accidental code change snuck into a `mv`. This catches the one class of error that matters at this stage.

**Localnet + ALT plumbing.** A `scripts/localnet.sh` that launches `solana-test-validator` with the correct Anchor program IDs preloaded, creates the USDC mint and test token accounts, and establishes a minimal ALT containing the kernel program and `ProtocolConfig` PDA. ALTs are used from L1 onward — creating the plumbing now means the kernel program's first localnet test writes v0 transactions from day one rather than retrofitting v0 later. Retrofitting v0 transactions late is painful in ways that aren't obvious until you hit the 32-account limit on the flagship in L4 and realize every localnet test needs rewriting.

**Anchor version pin.** Pick a single Anchor version (suggest: latest stable at the moment L0 starts) and pin it in every program and keeper `Cargo.toml`. Do not let Anchor version drift across programs — the IDL schema changes between versions and mismatched IDLs are a debugging time-sink.

### 4.2.2 What is not built

No instructions. No keepers. No tests beyond the backtest replay and `anchor build`. No frontend. No on-chain state definitions beyond the bare `declare_id!`.

In particular, don't start writing account structs for `ProtocolConfig` or `VaultState` "to save time in L1". Account structs are where Anchor's derive macros interact with byte layouts, discriminators, and account ordering — getting them half-right in L0 and rewriting them in L1 is worse than waiting. L1 writes them from scratch against Part 2's state topology.

### 4.2.3 Exit criterion

A single machine check: fresh clone of the repo → `make bootstrap` → `cargo test --workspace` → `anchor build` → the existing `make sol-autocall` backtest replay → all pass. The three product quote crates compile. The four program crates compile to empty BPF. The five keeper crates compile to empty binaries. `solana-test-validator` runs via the localnet script with the registered program IDs present.

If any of those fail, L1 doesn't start. The pathology of skipping this gate: L1 writes the kernel program, which CPIs into product stubs, which pull in quote crates that still have the wrong module boundaries, and you end up refactoring the crates with the kernel program pointed at them — which is exactly the entanglement the separation is designed to prevent.

---

## 4.3 Layer 1 — Kernel

**Goal.** A working `halcyon_kernel` program deployed to localnet, exercising the full lifecycle surface (reserve → finalize → observe → settle) against a stub product. Every kernel instruction in Part 2 has a handler, every kernel account has a layout, the mutual-CPI pattern is proven on a minimal case, and the ALT plumbing from L0 is exercised by a real transaction.

The kernel is built before any real product because three products will CPI into it, and having two products under construction against a moving kernel surface is how integration projects develop bugs that take days to bisect. Kernel surface freezes at the end of L1; products from L2 onward consume it as a dependency.

### 4.3.1 Work

**Kernel account structs and layouts.** Every PDA enumerated in 2.3 gets an Anchor `#[account]` struct: `ProtocolConfig`, `ProductRegistryEntry`, `VaultState`, `SeniorDeposit`, `JuniorTranche`, `PolicyHeader`, `CouponVault`, `HedgeSleeve`, `HedgeBookState`, `AggregateDelta`, `Regression`, `VaultSigma`, `RegimeSignal`, `FeeLedger`, `KeeperRegistry`, `LookupTableRegistry`.

Byte layouts are determined here. Document per-field widths in a `programs/halcyon_kernel/LAYOUTS.md` alongside the code — this is the struct-layout document that 2.14 defers to L1. Per the invariant in 3.3, every numeric field uses a specific integer type (`u64` for USDC/token amounts in their natural 6-decimal scale, `i64` for signed i64 s6 pricing quantities, `u128` only for intermediate overflow protection), and every struct has an explicit version discriminator at offset 0 so future upgrades can migrate in place.

**Kernel instructions.** Per Part 2, the kernel instruction surface is:

- Admin: `initialize_protocol`, `set_protocol_config`, `pause_issuance`, `pause_settlement`, `rotate_keeper`, `register_product`, `update_product_registry`, `register_lookup_table`, `update_lookup_table`.
- Capital: `deposit_senior`, `withdraw_senior` (with 7-day cooldown), `seed_junior` (admin-only at v1), `sweep_fees`.
- Oracle state: `update_ewma` (permissionless, rate-limited), `write_regression` (keeper-gated), `write_regime_signal` (keeper-gated), `write_aggregate_delta` (keeper-gated, flagship-only).
- Policy lifecycle: `reserve_and_issue`, `finalize_policy`, `apply_settlement`, `record_hedge_trade`.

Every handler follows the pattern in 3.3: authentication first, global pause check, capacity check, token transfer (if any), state mutation, account creation/update. `checked_add` and `checked_mul` throughout. Validation order is stable and documented in comments per handler.

**Stub product.** A minimal `halcyon_stub_product` program that exists only during L1 (deleted or moved to `research/` at the start of L2). It has three instructions:

- `accept_quote_stub` — takes a premium and a max liability, CPIs into `reserve_and_issue`, writes a trivial `ProductTerms` account with a single `u64 magic` field.
- `settle_stub` — CPIs into `apply_settlement` with a fixed payout.
- `init_terms_stub` — the callback the kernel invokes during the mutual-CPI finalize step.

The stub's purpose is to prove every kernel seam works end-to-end before any real pricer is wired up. It exercises the full happy path (issuance → active → settled) and the full failure path (issuance on a paused product → rejection). The stub is not the SOL Autocall product "with details omitted"; it's deliberately trivial so kernel bugs surface as kernel bugs rather than as "something in the pricer".

**Localnet tests.** Twelve tests, sized to exercise the surface without ceremony. An illustrative (not exhaustive) subset:

1. Fresh protocol initialize → ProtocolConfig exists with expected defaults.
2. Register stub product → ProductRegistryEntry visible, product_authority PDA recorded.
3. Senior deposit → SeniorDeposit account created, VaultState.total_senior updated.
4. Senior withdraw within cooldown → fails with expected error.
5. Senior withdraw past cooldown → succeeds, VaultState updated.
6. Happy-path issuance: stub calls `accept_quote_stub` → kernel CPI into `reserve_and_issue` → kernel CPI back into `init_terms_stub` → PolicyHeader status transitions Quoted → Active atomically.
7. Issuance while `issuance_paused_global` → fails with PausedGlobally.
8. Issuance above capacity → fails with CapacityExceeded.
9. Settle happy path: stub calls `settle_stub` → kernel CPI into `apply_settlement` → buyer receives clamped payout, unused reservation returned to free capital, PolicyHeader.status = Settled.
10. Settle while `settlement_paused_global` → fails.
11. Replay of the same settle call → fails (PolicyHeader not in Active state).
12. ALT-based v0 transaction: issuance path via v0 with lookup table resolving kernel + config + registry entry → succeeds.

Every test is written against the Anchor TypeScript test harness. The goal is not test coverage; it's to have a test the next layer can grep for when it breaks something.

**Event schemas.** Every kernel instruction that mutates money or status emits an event: `PolicyIssued`, `PolicySettled`, `HedgeBookUpdated`, `KeeperRotated`, `ConfigUpdated`, `FeesSwept`. Schema definitions live in `crates/halcyon_common/src/events.rs` so keepers and frontend can import them directly. This is the schema 2.14 defers; it's specified here because L2's observation keeper subscribes to these events.

### 4.3.2 What is not built

No real product handlers. No pricer integration (the stub doesn't call any quote crate). No keeper authorities for observation/hedge — those get registered in L2+ per product, not up-front. No frontend. No mainnet or devnet deploy — localnet only.

No `deposit_junior` public instruction. At v1, the junior tranche is founder-seeded via `seed_junior` which is admin-only. Public junior deposits open post-v1 and would introduce a whole withdrawal-timing and accrual-marker surface this layer doesn't need.

### 4.3.3 Exit criterion

The twelve localnet tests pass, reliably, across ten consecutive invocations.

The issuance CPI pattern is the two-forward-CPI shape documented in §2.10 (product → `reserve_and_issue`, product writes `ProductTerms` locally, product → `finalize_policy`). There is no kernel→product callback; earlier drafts of §2.10 that described one are superseded. The exit criterion replaces the "re-entrance panic" test (which was specific to the superseded callback pattern and does not apply here) with four explicit tests the L1 suite must cover:

1. **Happy path.** `accept_quote` drives `PolicyHeader` through `Quoted → Active`; `PolicyHeader.product_terms` points at the correct PDA; `PolicyHeader.terms_hash` matches `sha256(product_terms.account_data)`; `vault_state.total_reserved_liability` and `product_registry_entry.total_reserved` both increase by `max_liability`.
2. **Atomicity.** Product aborts between `reserve_and_issue` and `finalize_policy` (panic / early return). Transaction rolls back; `PolicyHeader` does not persist; reservations return to their pre-call values.
3. **Terms-binding enforcement.** `finalize_policy` called with a `product_terms` account whose bytes do not hash to `PolicyHeader.terms_hash` fails with `TermsHashMismatch` and the transaction rolls back.
4. **Status-machine integrity.** `finalize_policy` on a `PolicyHeader` that is not in `Quoted` (already `Active`, already `Settled`, or absent) fails with `PolicyNotQuoted`.

Anchor's account-constraints macro is still a source of seam gotchas — the specific LEARNED.md bug is the Anchor 0.32.1 seed-constraint aliasing issue on kernel-owned `Account<T>` passed across a product→kernel CPI. That bug is real, is documented in `LEARNED.md`, and is guarded by `scripts/check_cpi_seeds.sh`. Any subsequent Anchor gotcha encountered during L1 is appended to `LEARNED.md` before L2 begins.

The `LAYOUTS.md` document exists and matches the compiled IDL — if it drifts, every subsequent layer pays integration cost because keeper code and frontend code build against the IDL but humans reason against LAYOUTS. `make layouts-check` is a useful check to run at layer boundary and is wired into CI.

---

## 4.4 Layer 2 — SOL Autocall end-to-end

**Goal.** A full product working on devnet against live Pyth SOL/USD: user issues a policy via `accept_quote`, the observation keeper records observations every two days, autocall or final settlement clears the policy, the hedge keeper rebalances against Raydium. Every seam from Part 3 is exercised by a real transaction. No frontend yet — operator interaction is via a CLI.

This is the product that proves the template. SOL Autocall has no xStocks counterparty, no equity-feed Pyth availability concerns, no regression keeper, no Merkle-audited delta keeper. It exercises `preview_quote`, `accept_quote`, `record_observation`, `settle`, the observation keeper, the hedge keeper, and the Jupiter swap path, with the fewest concurrent risks. Everything learned here is directly reusable for IL in L3 and the flagship in L4.

### 4.4.1 Work

**`halcyon_sol_autocall` program.** Four public instructions:

- `preview_quote` — per 3.1 and 2.4. Read-only (called via `simulateTransaction`), returns `{premium, max_liability, quote_slot}`. Calls `solve_fair_coupon_at_vol` from `halcyon_sol_autocall_quote`. Applies the 75% quote share and 50 bps issuer margin per the product's economics. CU budget: benchmark at layer close, document in `programs/halcyon_sol_autocall/CU_BUDGET.md`.
- `accept_quote` — the full path: oracle reads, pricer call, slippage check against `max_premium`/`min_max_liability`, CPI into `reserve_and_issue`, kernel callback into `init_terms`, writes product-specific `ProductTerms` (entry price, full schedule, lockout flag, observation index = 0). Pricer entry point is `price_autocall_v2_parity`, which dispatches to the POD-DEIM live-operator engine when sigma is inside the `[50%, 250%]` training band (the primary path per `sol_autocall_math_stack.md` §4) and falls back to the gated Richardson CTMC otherwise (§5). If the fallback path reports low confidence (coarse/fine disagreement > 10%) the instruction aborts with a specific error rather than selling the user a policy at a price that may be wrong.
- `record_observation` — keeper-triggered every 2 days. Reads Pyth SOL at scheduled observation time, updates `ProductTerms.current_observation_index`, checks autocall (above 102.5%) subject to the first-observation lockout per `no_autocall_first_n_obs = 1` in the math stack, checks KI breach (below 70%), accumulates or pays coupon, and on autocall or maturity CPIs into `apply_settlement` in the same transaction. Emits `ObservationRecorded` and, if autocall, `PolicyAutoCalled`.
- `settle` — maturity settlement if no autocall fired earlier. Computes final payout per the product rules (coupon + notional if no KI ever breached; recovery based on ending ratio if KI breached), CPIs into `apply_settlement`.

The delta surface computation from 7 of `sol_autocall_math_stack.md` is consumed by the hedge keeper, not called from on-chain code. On-chain code handles the lifecycle; off-chain hedge replanning happens in the keeper using the same Rust code (compiled to native for the keeper).

**Keeper-authority wiring.** Register the observation keeper authority and hedge keeper authority for SOL Autocall in `KeeperRegistry` via the admin path from L1. Keepers sign with their registered keypair; the kernel and product both check against the registry.

**Observation keeper.** `keepers/observation_keeper/` grows to have SOL-Autocall-specific code: it watches the on-chain schedule, wakes on the correct slots, reads the Pyth price, builds and signs the `record_observation` instruction. It is idempotent (per 3.7) — calling it twice for the same observation is a no-op on the second call. Runtime: a small binary run as a systemd service or a serverless cron. Logging goes to stdout with structured JSON for later migration to a real monitoring path.

**Hedge keeper.** `keepers/hedge_keeper/` implements the full SOL Autocall hedge loop per 3.5:

1. Read all active SOL Autocall `PolicyHeader`s (iterate program accounts filtered by product_program_id and status=Active).
2. For each note, reprice via `price_autocall_v2_parity` (POD-DEIM primary, Richardson fallback — same engine as `accept_quote`) at current Pyth SOL and VaultSigma, extract the value/delta surface, interpolate to current spot. Skip if the fallback path flags low confidence.
3. Apply the `delta_obs_050` policy via `compute_hedge_target` from `hedge_controller.rs` per note: initial 50% of note delta, 75% clip, 10% hedge band, 1% minimum trade — per `sol_autocall_product_economics_report.md` §5.
4. Sum per-note targets → aggregate target.
5. Compare against `HedgeBookState.current_sol_holdings`. If out of band, compute the trade, subject to `max_rebalances_per_day` cap.
6. Fetch a Jupiter quote with slippage set to 50 bps. Sanity-check the quote's output-amount is within 100 bps of the theoretical spot (defensive against Jupiter routing through weird pools). This is a USDC↔SOL spot swap against live Jupiter liquidity; no counterparty integration.
7. Execute the swap signed by the hedge keeper authority.
8. CPI into `record_hedge_trade` to write the new position onto `HedgeBookState`.

Cadence per `integration_architecture.md` §2.6: the keeper wakes on each observation event (every 2 days, 8 per 16-day note) with intraperiod band checks allowed under the `max_rebalances_per_day` cap. Target behaviour is ~4.5 trades per note averaged across the backtest. A manual wake knob is exposed for operator intervention.

**CLI.** A small Rust CLI `tools/halcyon_cli/` that, given a wallet keypair, can: initialize the protocol (one-shot), register SOL Autocall, seed the junior tranche, make a senior deposit, preview a SOL Autocall quote, accept a SOL Autocall quote, force-trigger the observation keeper once, force-trigger the hedge keeper once, and print the current state of all active policies. No frontend wraps this in L2; the operator is the user.

**Devnet deploy.** The SOL Autocall program, the kernel program, the stub product (removed), and the support crates are deployed to devnet against real Pyth SOL/USD. Halcyon mock-USDC is used for premium payments, created with `tools/mock_usdc_faucet`, wired with `init-payment-mint`, and exposed through `/faucet` so judges can self-fund. The keepers run from a dev machine or a cheap VPS against devnet RPC.

**End-to-end flow test.** A scripted run:

1. Operator (via CLI) makes a senior deposit of 10,000 mock-USDC.
2. Operator seeds the junior tranche with 1,000 mock-USDC.
3. Operator issues a SOL Autocall policy for 500 mock-USDC notional, 16-day tenor.
4. Clock warps (in a test harness) or 16 real days elapse. Every 2 days the observation keeper fires.
5. Depending on how SOL moves against devnet Pyth: autocall fires → settlement. Or: natural expiry → settlement.
6. Hedge keeper fires at least twice across the lifetime.
7. Final state: PolicyHeader.status = Settled, HedgeBookState reflects all trades, FeeLedger shows the treasury fee.

The test passing against live Pyth, live Raydium, and two weeks of wall clock is the gate. Simulated-clock testing is necessary but not sufficient — the real-time observation cadence often surfaces ordering issues that simulated clocks hide.

### 4.4.2 What is not built

No IL Protection. No flagship. No regression keeper (SOL Autocall doesn't use the IWM-regression projection). No delta keeper with Merkle commitments (SOL Autocall's hedge keeper computes per-note delta on demand). No frontend.

No junior depositor public flow — still admin-only.

Don't build the observation keeper as a general "all products" keeper in L2. It's SOL-Autocall-specific in this layer. The generalisation happens in L3 when IL Protection needs its own observation (well — it doesn't, it's European-settled, but the keeper structure gets reused for the settlement-triggering role).

### 4.4.3 Exit criterion

A full SOL Autocall lifecycle completes on devnet, unattended, over a real 16-day period, against live Pyth and live Raydium. The operator issues via CLI; every subsequent state transition happens by keeper. No manual interventions are required. The terminal state is consistent: PolicyHeader settled, HedgeBookState reflects all trades, VaultState accounting balances, FeeLedger shows the expected treasury fee to the USDC.

The CU benchmarks for `accept_quote`, `record_observation`, and `settle` are documented and within envelope (per Part 2, well below the 1.4M CU transaction limit — SOL Autocall's pricer is the cheapest of the three).

Integration notes: any subtle seams that bit during L2 — mutual-CPI ordering gotchas, Pyth staleness edge cases, Jupiter routing anomalies, specific Anchor version issues — are documented in a `LEARNED.md` at repo root. L3 and L4 consult this document before they write their equivalents.

---

## 4.5 Layer 3 — IL Protection

**Goal.** Second product live on devnet. IL Protection is European-settled (no observation keeper in the mid-life sense), is unhedged (no hedge keeper), but requires a regime keeper that computes fvol off-chain and writes it on-chain. The value of L3 is not the IL product itself — it's proving the kernel surface and keeper architecture scale to a second product with different lifecycle shape.

This layer is shorter than L2 because most of the integration patterns are established. The new work is product-specific pricer wiring, a new keeper with a novel compute profile, and settlement against a Raydium CPMM pool.

### 4.5.1 Work

**`halcyon_il_protection` program.** Three public instructions:

- `preview_quote` — reads `VaultSigma` and `RegimeSignal` (for the sigma multiplier), reads Pyth SOL and USDC prices, calls `halcyon_il_quote::price_il_protection`, applies product spread. No changes to the pattern established in L2.
- `accept_quote` — same pattern: slippage check, kernel CPI, `ProductTerms` write. `ProductTerms` is simpler than SOL Autocall's — pool descriptor, entry prices, expiry, regime snapshot. No mid-life observation state.
- `settle` — at expiry only. Computes exit SOL/USDC from Pyth, computes the IL deductible/cap payoff per the product spec, CPIs into `apply_settlement`. Can be called by anyone after expiry (no keeper trust needed for settlement — the math is fully determined by on-chain state + oracle reads).

No observation instructions. No hedge keeper. This is the architectural benefit of the IL product — it consumes margin from the vault directly per 6.2 of the economics report, which means no hedge counterparty risk but higher demands on vault capitalization.

**Regime keeper.** `keepers/regime_keeper/` is new. Its compute profile differs from the other keepers: it doesn't respond to events, it runs on a fixed daily cadence (per `integration_architecture.md` §2.8, after the Pyth daily-close window), and it computes fvol from a rolling historical SOL return window. The computation is not on-chain (fvol requires the full historical series), so the keeper does it off-chain and writes the result via `write_regime_signal` — which sets the regime (calm if fvol < 0.60, stress if fvol ≥ 0.60), the sigma multiplier (×1.30 calm, ×2.00 stress), and the 40% sigma floor per `il_protection_product_economics_report.md` §4. The on-chain write is rejected if the previous `RegimeSignal` is less than 18 hours old. The keeper's authority is narrowly scoped: it can only write `RegimeSignal` accounts, nothing else.

Implementation note: fvol computation uses the same Rust module as the economics explainer references. The keeper links `halcyon_il_quote` (which owns this module) and runs the computation natively against historical data fetched from a simple REST endpoint (CoinGecko or similar for demo; production should move to a more robust source).

**Settlement math verification.** The payoff formula in `il_protection_math_stack.md` is implemented in `halcyon_il_quote::settlement` and exercised by unit tests against Python reference values from the backtest (same fixtures the replay uses). On-chain settlement calls this function — no reimplementation, same source of truth. A devnet settlement is validated against a parallel Python reference: same inputs, same output to the basis point.

**CLI extensions.** `tools/halcyon_cli/` gains IL-specific subcommands: preview an IL policy, accept an IL policy, check the current regime signal, force-trigger the regime keeper.

**End-to-end test.** A 30-day IL policy is issued, runs to maturity, and settles correctly against a plausibly-moving devnet Pyth SOL/USDC. The regime keeper wakes daily, writes its signal, and is respected at issuance time. The settlement payoff matches a Python reference implementation to a basis point.

### 4.5.2 What is not built

No hedging for IL — it's unhedged by design. No Raydium LP token escrow — the product is synthetic (settles against Pyth, not against real LP positions), matching the `il_protection_math_stack` and `page_il.jsx` synthetic path. The "LP path" shown in the demo frontend's state machine is a demo fiction in L3; if it becomes real it's a post-v1 feature.

No flagship. No frontend still.

### 4.5.3 Exit criterion

A 30-day IL policy lifecycle completes on devnet. The regime keeper has written at least 30 daily `RegimeSignal` updates. Settlement payoff matches the Python reference within 1 bps. The FeeLedger reflects the correct treasury fee.

Integration-cost observation: L3 took substantially less calendar time than L2 even though a keeper was added. This is the replication dividend — the kernel surface and seam patterns paid off. If L3 took as long as L2, something is wrong with the shared infrastructure; pause and fix it before L4.

---

## 4.6 Layer 4 — Flagship

**Goal.** The flagship worst-of-3 equity autocall on SPY/QQQ/IWM, live on devnet, with all three keepers (observation, regression, delta) operational, the hedge keeper executing USDC↔SPYx and USDC↔QQQx spot swaps through Jupiter (or a mock pool if mainnet Jupiter routes for SPYx/QQQx are not accessible from devnet), and both correction tables (K=12 and daily-KI) loaded with their SHA-256 commitments verified at program startup.

This is the longest layer because it carries the most new surface: three keepers (not one), two correction tables (not zero), a Merkle-audited aggregate delta, a regression keeper, and the IWM projection to a 2D hedge. There is no xStocks counterparty integration — SPYx and QQQx are ordinary SPL tokens issued by Backed Finance that the hedge sleeve holds and trades through whatever DEX liquidity Jupiter routes to. At the v1 demo scale ($50–$500 positions) called out in `integration_architecture.md` §2.13, no primary-market Backed relationship, sandbox, or API integration is required; the only external check that matters is Jupiter route depth for SPYx/QQQx pre-launch.

L4 is also where the calendar-bound external work matures: Pyth SPY/QQQ/IWM feed verification on devnet and mainnet, Jupiter route-depth check for SPYx/QQQx at demo notional, legal-counsel engagement for the securities question. These items should have been in motion since L0; L4 is where they land into the build.

### 4.6.1 Work

**Correction tables.** Both correction tables ship as compile-time constants in `halcyon_flagship_quote`:

- K=12 correction: generated from the existing backtest harness. Already exists per Part 1.4.
- Daily-KI correction: per 1.4 and 2.5, this is re-derived 3D Fang-Oosterlee COS at 256 σ points, committed as canonical JSON with a SHA-256 hash. If this table isn't already generated, generating it is L4 work and is the single most compute-heavy item in the whole build. Regenerate it off-chain, commit the bytes to the crate, commit the JSON source to `research/daily_ki_correction/` for auditability.

Both tables' SHA-256 hashes are written into `ProtocolConfig` at the flagship's registration time. The program checks the compiled-in hash against the stored one at the start of every pricing instruction and aborts with `CorrectionTableHashMismatch` if they diverge. This is the verifiability path named in 2.13 — a future program upgrade that changes the tables requires updating the ProtocolConfig hash, which the admin multisig signs.

**Factor-model constants.** `FLAGSHIP_FACTOR_MODEL` is a compile-time constant per 3.4, sourced at build time from `output/factor_model/spy_qqq_iwm_factor_model.json`. A `build.rs` in `halcyon_flagship_quote` reads the JSON and emits a Rust module with the constants. Recalibration means updating the JSON and redeploying the program — not a runtime config.

**`halcyon_flagship_autocall` program.** The instruction surface is larger than SOL Autocall:

- `preview_quote` — reads VaultSigma, Regression, three Pyth feeds (SPY, QQQ, IWM), calls the three-step pricer: `quote_frozen_k12` → `k12_correction_lookup` → `daily_ki_correction_lookup`. Per 3.4.
- `accept_quote` — same pattern, with three-feed staleness check.
- `record_coupon_observation` — monthly (18 total). Reads worst-of on the three feeds against entry. Updates memory-coupon state.
- `record_autocall_observation` — quarterly (6 total). The filter's cadence. On autocall, CPIs into `apply_settlement`.
- `record_ki_event` — daily, fires only when any name closes below 80%. Sets the KI-latched flag in `ProductTerms`.
- `settle` — at maturity. Computes the final payoff per the worst-of autocall rules.

The trilevel observation cadence from 2.3 is the main product-specific complexity here. Getting the schedule arithmetic right (TradingDays vs. calendar days, holiday handling, Pyth feed hours) matters and needs testing against both 20 years of historical data (via the existing backtest) and current-year forward schedules.

**Regression keeper.** Writes `Regression` on a daily cadence (per `integration_architecture.md` §2.8) with a 5-day on-chain staleness cap on the resulting account for downstream instructions. Computes the 252-day rolling OLS regression of IWM onto SPY and QQQ from historical equity data (off-chain history provider: Polygon, Databento, or IEX — single documented dependency), sanity-checks the latest close against Pyth (abort on divergence > 1%), produces `β_SPY`, `β_QQQ`, `α`, `r²`, `residual_vol`, window timestamps, sample count. Same off-chain compute pattern as the regime keeper but with a richer payload. Authority narrowly scoped to `write_regression`.

**Delta keeper with Merkle commitment.** The most novel keeper. Per 2.13 and 3.6:

1. Fetch all live flagship policies.
2. For each, run the analytical gradient path in `halcyon_flagship_quote::worst_of_c1_filter_gradients` against current Pyth prices.
3. Compute per-note 3D delta.
4. Build a Merkle tree over `(note_pubkey, delta_vector)`.
5. Aggregate sum → `AggregateDelta`. Include the Merkle root, the Pyth snapshot used, timestamp, and note count.
6. CPI into `write_aggregate_delta`.
7. Publish the per-note delta list to an off-chain store (S3 or similar) at a URL whose hash is included in the on-chain Merkle root's commit log.

The auditability claim is: any party with the per-note delta list can reconstruct the Merkle tree and verify the aggregate matches. Separately, any party with `ProductTerms` + factor-model constants + Pyth snapshot can re-run the analytical gradient path and verify each per-note delta independently. Both verification paths use only code that ships open-source in the quote crate.

Delta keeper cadence: 15-30 seconds during market hours, dual-triggered on Pyth price-change events (per 2.6). Market-hours gating: if SPY/QQQ/IWM feeds are stale (outside equity market hours), the keeper sleeps.

**Hedge keeper (flagship-specific).** Different from SOL Autocall's hedge keeper because it reads `AggregateDelta` rather than recomputing per-note, and because the flagship is worst-of three underlyings (SPY, QQQ, IWM) but only two of them have on-chain wrappers with meaningful Jupiter liquidity (SPYx, QQQx). The vault therefore hedges the SPY and QQQ deltas directly and **projects the IWM delta onto the SPY and QQQ legs** using a rolling 252-day OLS regression of IWM daily log-returns onto SPY and QQQ daily log-returns (written on-chain by the regression keeper as `β_SPY`, `β_QQQ`, `α`, `r²`, `residual_vol`). Per `worst_of_autocall_product_economics_report.md` §5, the historical fit is approximately `IWM ≈ 1.14 · SPY − 0.01 · QQQ` with R² ~80% (mean) / ~66% (P10) and residual vol ~9.8% annualised; roughly 80% of IWM's move is absorbed into the SPY/QQQ hedge legs and the remaining ~20% small-cap-specific residual is retained as variance. The explicit 2.6 target:

```
target_SPY = Δ_SPY + β_SPY · Δ_IWM
target_QQQ = Δ_QQQ + β_QQQ · Δ_IWM
```

Cost of the proxy hedge versus a hypothetical direct 3-leg hedge is ~70 bps of occupied-capital return (quantified in the economics report); this is accepted explicitly so that the vault takes no third-wrapper dependency. If an IWM wrapper with sufficient Jupiter liquidity appears post-v1, upgrading to a 3-leg hedge is a keeper-config change, not a program change — the hedge keeper already consumes `Δ_IWM` from `AggregateDelta` and only decides how to route it.

The keeper then executes USDC↔SPYx and USDC↔QQQx spot swaps through Jupiter (or against a mock SPYx/QQQx pool on devnet if mainnet routes aren't devnet-accessible — in which case the architecture is the same but the liquidity source differs). No Backed Finance API. No primary-market allocation flow. The hedge sleeve accumulates and sheds SPYx/QQQx purely through Jupiter routes like any other SPL token pair; this is the whole point of on-chain wrappers.

Rebalance gates per 2.6: 5-day cadence floor (4-day on-chain minimum gap between rebalances) or event-triggered on `|Δ_aggregate(now) − Δ_aggregate(last)| > 1.5 × band_width`. 30-minute or 0.5%-move staleness cap on the delta account. Jupiter-slippage sanity before execution — output amount must be within 100 bps of Pyth-implied spot, else abort and try again next cycle.

**End-to-end flagship test.** A condensed test on devnet:

1. Operator issues one flagship policy.
2. Regression keeper writes `Regression` (use a historical window for the initial compute; production schedule is daily, 5-day on-chain staleness cap).
3. Delta keeper wakes every 30 seconds, writes `AggregateDelta` with a Merkle root of one note.
4. Hedge keeper wakes, reads `AggregateDelta`, computes the IWM-projected 2D target, decides whether to rebalance.
5. If the test doesn't wait 18 months for natural expiry: use an admin path or a time-warp on devnet to advance observation schedules and trigger the expected cash-flow events.
6. End state: PolicyHeader settled, HedgeBookState reflects trades, AggregateDelta's Merkle root can be verified against the published per-note list.

### 4.6.2 What is not built

No mainnet deploy yet. No frontend yet.

No multi-wrapper-issuer path for xStocks. V1 holds SPYx and QQQx as Backed-Finance-issued SPL tokens (the only xStocks wrappers with material Jupiter liquidity today), routed through Jupiter like any other token. Multi-issuer routing and Backed primary-market integration are both post-v1 concerns — not built, not required at demo scale.

No IL-like synthetic version of the flagship. If Jupiter SPYx/QQQx liquidity proves insufficient for live hedging on mainnet at the demo notional, a synthetic version (unhedged, higher vault margin) is a potential v1.5 path — but L4 does not build it.

### 4.6.3 Exit criterion

A flagship policy lifecycle completes on devnet with all three keepers live. The Merkle root on the most recent `AggregateDelta` account verifies against the published per-note list. Correction table hashes match the `ProtocolConfig` commits. The hedge keeper has executed at least two rebalances (one initial, one event-triggered) round-tripping USDC↔SPYx and USDC↔QQQx through Jupiter (or the devnet mock pool). CU benchmarks on the flagship's pricing path are within the 955K CU budget target from 2.5.

Calendar-item readiness: by end of L4, the legal-counsel engagement has produced a clear v1 issuance path (either: regulated distribution partner secured, or: limited geography / institutional-only for v1), Pyth SPY/QQQ/IWM feeds have been verified live on mainnet within staleness tolerance, and Jupiter mainnet routes for SPYx/QQQx have been confirmed to support the demo-scale book ($50–$500 positions). No Backed Finance relationship is required at this stage — the operational dependency is only Jupiter route depth.

If any calendar item has not landed by end of L4, L5 still starts but ships flagship in a "paused at issuance" state on mainnet — SOL Autocall and IL are live, flagship is deployed but not accepting policies until the calendar items clear.

---

## 4.7 Layer 5 — Frontend and Mainnet Readiness

**Goal.** Replace the demo `app/` with a production Next.js frontend that a retail user can transact through. Complete mainnet readiness: audit, multisig ceremony, monitoring, circuit-breaker drills, runbook. Deploy to mainnet with at least one product live.

The frontend is deferred to L5 rather than built alongside L2/L3/L4 for a specific reason: frontend code built against an unstable IDL is rewritten repeatedly. Building frontend last, when all three product IDLs are stable, means the frontend is written once rather than three times. The L0–L4 operator experience via CLI is slower but has lower integration cost.

### 4.7.1 Work

**Frontend.** Next.js + Anchor IDL + wallet adapter per 3.8. Four user-facing pages:

- **Flagship issuance** — replaces `page_equity.jsx`. Preview quote, slippage tolerance control, sign and submit `accept_quote`.
- **SOL Autocall issuance** — replaces `page_sol.jsx`. Same pattern.
- **IL Protection issuance** — replaces `page_il.jsx`. Drops the state-machine complexity of the demo (no LP / no-LP / synthetic distinction — v1 is synthetic-only per L3).
- **Portfolio** — replaces `page_portfolio.jsx`. Reads user's active policies across all three products via `getProgramAccounts` with product program ID filters.

One vault page (replaces `page_vault.jsx`) showing aggregate vault state, but no deposit flows beyond admin. Public senior deposits open in v1.1.

Design notes:

- Every transaction is v0 with the appropriate product-specific ALT — no legacy transactions.
- Types come from the Anchor IDL via `@coral-xyz/anchor` — no hand-rolled account deserializers, per 3.8.
- Preview is `simulateTransaction` against the program's `preview_quote`, not a JavaScript reimplementation of the pricer. Single source of truth preserved.
- Slippage tolerance is user-editable, defaults to 50 bps.
- No indexer dependency for v1. `getProgramAccounts` + Anchor filters is sufficient at v1 scale. Post-v1, if account enumeration latency becomes material, add an indexer — but not before then.

**Audit preparation.** This deserves weeks, not days. Specific items:

- Freeze all four programs. No functional changes post-freeze except audit-driven fixes.
- Produce one comprehensive `ARCHITECTURE.md` and one `THREAT_MODEL.md` handed to auditors.
- Runbook of known open questions from the Part 5 tensions discussion (which is not yet written, but every named tension here goes on the list).
- Unit-test and integration-test coverage snapshot. Gaps get filled before submission, not after.

Depending on audit turnaround and whether one or two auditors review in parallel, this can take 4–8 calendar weeks. It runs partially in parallel with the frontend work — audit kickoff is at the start of L5, not the end.

**Mainnet ceremony.**

- 3-of-5 or 4-of-7 multisig for `admin_authority` on `ProtocolConfig`. Keys held on distinct hardware in distinct geographies. The multisig setup rehearsal happens on devnet first — every admin instruction gets exercised via the multisig before mainnet deploy so unfamiliar Squads/Cube UX doesn't cause a bad mainnet moment.
- Keeper authority keypairs are generated fresh for mainnet, rotated from devnet. Keeper infrastructure (VPS, monitoring, alerting) is provisioned separately.
- The initial junior seed is executed.
- Circuit breakers (global pause flags) are tested on mainnet against an issuance immediately after deploy: pause → attempt to issue → rejection → unpause → successful issuance. No real user sees this; it's a smoke test.

**Monitoring.** At minimum:

- RPC health per endpoint used by each keeper.
- Per-keeper heartbeat (last-run timestamp, last-success timestamp).
- Pyth feed staleness dashboard — all five feeds (SPY, QQQ, IWM, SOL, USDC).
- Vault utilization dashboard.
- Per-policy status summary.
- Alerting on: any keeper stale > 2× its expected cadence, any Pyth feed stale > its staleness cap, vault utilization > 85%, any failed `apply_settlement` (suggests a settlement-math edge case).

Hosting: any infrastructure from Grafana Cloud to a self-hosted Prometheus+Grafana stack is fine. The bar is "if something breaks at 3am, the alert wakes someone".

**Mainnet deploy.** Deploy all four programs. Register all three products but set the flagship to `paused` if the legal or Jupiter-route-depth calendar items haven't landed per L4. Open issuance for the active products. First few policies are operator-issued for smoke-testing (notional capped low per 2.13's `$50–$500 demo scale` guidance); then external users.

### 4.7.2 What is not built

No v1.1 features: no junior public flow, no multi-issuer xStocks, no indexer, no advanced portfolio analytics, no mobile app, no additional products from the whitepaper roadmap (caps, floors, range accruals, twin-wins).

No ecosystem SDK beyond `halcyon_client_sdk`. Third-party integrations are post-v1.

### 4.7.3 Exit criterion

Mainnet deploy succeeds. Multisig control of `admin_authority` is confirmed via a live admin instruction. At least one product is open for issuance, with keepers live, monitoring green. Audit report is delivered and all critical/high findings are either remediated in code or explicitly accepted with documented justification. First external user (not the founder) successfully completes a policy lifecycle.

The protocol is now live. Iteration from here is operational — product tuning, expanding the shelf, distribution — not integration.

---

## 4.8 Parallel tracks

Three streams of work run in parallel to the layered build and feed into it:

**Legal and distribution track.** Starts at the beginning of L0, not L4. The sequence: initial counsel engagement (structured products on-chain, US-securities analysis on flagship, international perimeter analysis), scoping memo on v1 issuance geography, distribution partner outreach, distribution partner due diligence, signed distribution agreement. This realistically takes 8–12 weeks of calendar time and running it in parallel is the only way it doesn't block mainnet.

Outputs that feed into L4 and L5: the geography/jurisdiction scope for v1 (determines what frontend geoblocks, if any, and what the issuer-margin structure needs to accommodate), the distribution partner's technical requirements (KYC integration, reporting hooks, if any), a clear statement of what v1 can and cannot sell to whom.

**Jupiter route-depth and Pyth equity feed track.** Starts at L2, not L4, because the check is cheap and the answer informs the flagship's mainnet go/no-go. The sequence: probe mainnet Jupiter quotes for USDC↔SPYx and USDC↔QQQx at demo notional ($50, $250, $500) to confirm slippage stays inside the 100 bps Pyth-sanity band; probe Pyth SPY/QQQ/IWM mainnet feeds for publish cadence and staleness under live market hours; document both in `research/prelaunch_route_checks/`.

There is no Backed Finance operational track at v1. SPYx and QQQx are treated as ordinary SPL tokens on the Jupiter router; the hedge sleeve acquires them by swapping USDC at rebalance time, exactly the same pattern the SOL Autocall hedge sleeve uses for USDC↔SOL. The primary-market Backed flow (creating new wrapper units against direct equity purchases) is a scale concern for post-v1 when the flagship book outgrows secondary-market DEX depth; at demo scale it is explicitly not in scope per `integration_architecture.md` §2.13.

Feeds into L4's hedge keeper implementation and L5's go/no-go on the flagship's mainnet issuance opening.

**Daily-KI correction table regeneration.** If not already shipped in the existing repo, this is a dedicated compute-heavy subtrack that runs through L0–L4 and needs to complete before L4. 56-hour compute on a 4-worker setup per `worst_of_math_stack.md` 7.4; real calendar time including iteration and validation is probably 2–3 weeks. Delegating this to a dedicated machine (or cloud compute) while the in-repo work proceeds is the right sequencing.

---

## 4.9 Test strategy, as deferred from 2.14

Part 2.14 explicitly deferred test strategy to the build-order document. Here it is, layer by layer.

**L0.** Existing backtest replay + `cargo test --workspace` + `anchor build`. No new test infrastructure; the bar is that nothing regresses.

**L1.** Localnet integration tests under Anchor's TypeScript harness. Twelve tests as enumerated in 4.3.1. These remain the kernel's regression suite for the rest of the build — every subsequent layer runs them in CI.

**L2.** Localnet tests for the SOL Autocall program's handlers (happy path per instruction, at least one failure path per instruction, mutual-CPI verification with the real product). Devnet integration test: a single policy lifecycle executed against live Pyth and live Raydium. Keeper unit tests in Rust, keeper integration tests via ephemeral local Pyth + local Raydium sim.

**L3.** Localnet tests for `halcyon_il_protection`. Python cross-check of settlement math — a test that runs the Rust settlement function and the Python reference on the same inputs and asserts equality to a basis point. This is the pattern the Part 1 inventory flagged: "one source of truth for each thing" is validated by the absence of implementation drift between backtest and production code.

**L4.** Localnet tests for `halcyon_flagship_autocall`. SHA-256 commitment check of both correction tables. Merkle-root verification of `AggregateDelta` as a standalone test. CU benchmark as a test that fails if the worst-of pricing CU exceeds the documented budget.

**L5.** End-to-end browser-driven tests against devnet (Playwright or similar). One scripted issuance per product. Slippage rejection paths tested. Wallet adapter disconnection recovery tested. Mainnet smoke tests executed post-deploy as documented above.

**Continuous.** Every layer's tests run in CI on every commit from the layer they were introduced onward. A layer that breaks an earlier layer's test blocks merge.

---

## 4.10 What changes this plan

Three things would cause the layer order or content to change materially, and they should be watched for during the build:

- **Jupiter SPYx/QQQx route depth proves inadequate for the flagship's demo notional.** If the route-depth probes during the L2–L3 parallel track show that a $500 USDC↔SPYx swap can't clear the 100 bps Pyth-sanity band, L4 ships the flagship in a synthetic configuration (unhedged, higher vault margin per the IL Protection pattern) rather than skipping it. The crate refactor in L0 already separates the flagship pricer from the hedge controller, so this substitution is not a code rewrite — it's a deploy-time choice about which keeper set runs.

- **Pyth equity feeds are unavailable or unreliable on devnet.** If L4 can't test the flagship's observation path against live feeds, fall back to a mock Pyth publisher on devnet that replays historical data. The mock is not production code; it lives in `research/devnet_mocks/` and is only linked against devnet test builds. This preserves L4's exit criterion (observation lifecycle complete) without blocking on external infrastructure.

- **Legal scope forces v1 to drop the flagship entirely** (for example, securities-registration path is infeasible in acceptable timeframe). In this case L4 becomes a smaller layer — no flagship — and L5 opens mainnet with only SOL Autocall and IL Protection. The protocol still ships. Flagship moves to v1.5 when the regulated path clears. This is the single biggest scope-risk and the whitepaper's Section 9 already names it; the build should not pretend it might not happen.

None of these materially affects layers L0–L3. They're L4-specific. The build order is robust to each of them independently.

---

*End of Part 4. Parts 5 (Known Tensions and Mitigations) and 6 (Integration Risk Register) to follow. Part 5 is most usefully written after L1 exit — the mutual-CPI tension is the first one that concretizes at that point, and writing the tensions document against real bug reports rather than anticipated ones gives it sharper teeth. Part 6 is written at L4 entry — the integration risks that matter for mainnet are mostly flagship-specific and are clearer against a working SOL Autocall and IL Protection than against a blank page.*
