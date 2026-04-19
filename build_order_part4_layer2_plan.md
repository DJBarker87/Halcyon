# Halcyon — Layer 2 (SOL Autocall end-to-end) Execution Plan

**Target audience:** a fresh Claude Code instance starting with zero prior context.
**Prerequisite:** L1 is complete (see "L1 state handoff" below). Do not start L2 until the L1 exit criterion (4.3.3 of `build_order_part4.md`) passes — twelve localnet tests green across ten consecutive invocations, `LAYOUTS.md` matches compiled IDL, `anchor build` produces five `.so` files, `make bootstrap` still works.
**Exit criterion (L2):** a full SOL Autocall lifecycle completes on devnet, unattended, over a real 16-day window, against live Pyth SOL and live Raydium. CU benchmarks documented. `LEARNED.md` updated with any new integration notes.

---

## 1. Context you need before you start

Read these in order. Do not skip.

1. `halcyon_whitepaper_v9.md` §5 + §6 — what the SOL Autocall product *is*. 16-day tenor, 8 × 2-day observations, 102.5% autocall barrier, 70% KI barrier, 2-day lockout, 50 bps issuer margin, 75% quote-share. Backtested 20.7% CAGR rolling-reinvest.
2. `integration_architecture.md` §2.1-2.4 (topology, state, lifecycle), §2.6 "SOL Autocall — per-note observation-rebalanced hedge with `delta_obs_050`", §3.1-3.7 (all seams relevant to a product + its keepers).
3. `build_order_part4.md` §4.4 — the L2 definition itself. Authoritative.
4. `LEARNED.md` — Anchor seed-constraint gotcha under product→kernel CPI (L1). **You must apply this pattern to every product→kernel CPI in L2.** Same rule: `Account<T>` only, no `seeds + bump` on kernel-owned PDAs received across a CPI boundary.
5. `build_order_part4_layer1_plan.md` — the L1 plan, so you know how L1 approached similar problems (instruction-surface granularity, `LAYOUTS.md` parity check, test harness pattern, twelve-test tradition).
6. `MEMORY.md` in the project memory store — who Dom is, and that the landing page pitches the wrong products (not L2 scope but don't build away from it).

When Parts 2, 3, and build-order diverge on a detail: `integration_architecture.md` wins. If silent, `build_order_part4.md` wins.

Pre-existing quote-crate-level references — skim, don't read cover-to-cover:
- `crates/halcyon_sol_autocall_quote/src/autocall_v2.rs` — FFT pricer, `solve_fair_coupon_at_vol`.
- `crates/halcyon_sol_autocall_quote/src/autocall_v2_parity.rs` — Richardson (`price_autocall_v2_parity`), low-confidence gate.
- `crates/halcyon_sol_autocall_quote/src/hedge_controller.rs` — `compute_hedge_target`, `delta_obs_050` policy.
- `crates/halcyon_sol_autocall_quote/src/autocall_hedged.rs` — backtest harness (reference only).

---

## 2. L1 state handoff

What exists in the repo right now (as of L1 exit):

### Workspace layout

```
crates/
  halcyon_common/                 PDA seeds, SCALE_6/SCALE_12 + to_scale_* helpers,
                                  HalcyonError, event schemas (PolicyIssued, PolicySettled,
                                  HedgeBookUpdated, KeeperRotated, ConfigUpdated, FeesSwept).
  halcyon_kernel_types/           AnchorSerialize mirrors of ProtocolConfig, VaultSigma,
                                  RegimeSignal, Regression, ProductRegistryEntry,
                                  PolicyHeader, KeeperRegistry; PolicyStatus enum.
  halcyon_oracles/                empty — grows in L2 (Pyth read helpers, staleness).
  halcyon_flagship_quote/         17-module flagship pricer. Unchanged from L0.
  halcyon_il_quote/               IL pricer. Unchanged from L0. `pool/*` is reference-only.
  halcyon_sol_autocall_quote/     SOL Autocall pricer + hedge controller. Unchanged from L0.
  halcyon_client_sdk/             empty — grows across L2-L5.
  halcyon-wasm/                   browser shim.
programs/
  halcyon_kernel/                 FULL L1 kernel:
                                    - 16 account structs (LAYOUTS.md)
                                    - 20 instructions: admin / capital / oracle / lifecycle
                                    - reserve_and_issue + finalize_policy split for mutual-CPI
                                    - record_hedge_trade, apply_settlement
                                    - errors: KernelError (kernel-local) + HalcyonError (shared)
  halcyon_stub_product/           L1-only stub used to exercise the kernel mutual-CPI seam.
                                  RELOCATE TO research/ AT START OF L2 (or delete — kernel keeps
                                  it in the IDL until you remove the workspace member).
  halcyon_flagship_autocall/      empty #[program] scaffold. L4 work.
  halcyon_il_protection/          empty #[program] scaffold. L3 work.
  halcyon_sol_autocall/           empty #[program] scaffold. THIS IS THE L2 TARGET.
keepers/
  observation_keeper/             empty main.rs.
  regression_keeper/              empty main.rs.
  delta_keeper/                   empty main.rs.
  hedge_keeper/                   empty main.rs.
  regime_keeper/                  empty main.rs.
tests/kernel/
  kernel.spec.ts                  12-test Anchor TS harness. DO NOT REMOVE — this is the
                                  kernel's regression suite for the rest of the build.
tools/                            not yet created — L2 adds `tools/halcyon_cli/`.
```

### Program IDs (already deployed via `declare_id!`, `Anchor.toml`, keypairs in `target/deploy/`)

- `halcyon_kernel`            = `H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF`
- `halcyon_stub_product`      = `BHjoWaj82FyupNLgHQTjBCfoNaED4HwQbR2KBNapht1d`
- `halcyon_flagship_autocall` = `E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc`
- `halcyon_il_protection`     = `HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g`
- `halcyon_sol_autocall`      = `6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8`

### Toolchain pins (unchanged from L1)

- `anchor-cli 0.32.1`, `anchor-lang = "=0.32.1"`, `anchor-spl = "=0.32.1"` everywhere.
- `solana-cli 2.3.0`, `cargo-build-sbf 2.3.0`, `platform-tools v1.48`.
- `blake3 = "=1.5.5"` pin in `Cargo.lock` (SBF rustc 1.84 predates `edition2024`, so `constant_time_eq 0.4` via `blake3 1.8` won't build). **Do not bump blake3.**

### Known L1-era quirks (cumulative with L0 quirks)

- **Anchor seed-constraint aliasing bug on CPI.** Kernel-owned PDAs passed through a product → kernel CPI must not carry `seeds = [...], bump` in the kernel's Accounts struct — this triggers a memory aliasing issue on Anchor 0.32.1 / SBF rustc 1.84 that silently zeroes fields after the first byte. Fall back to `Account<T>` discriminator validation only. Full writeup in `LEARNED.md`. **Apply this rule to every L2 instruction that the SOL Autocall product CPIs into.**
- L0's `halcyon-wasm` profile-in-package warning still emits; still harmless.
- `crates/halcyon_il_quote/src/pool/*` is still on disk but not declared in `lib.rs`. Leave it alone in L2.

### Verification before you start L2

Run every one of these. If any fails, fix it *before* writing a line of L2 code:

```
make bootstrap                   # L0 gate still green
anchor build                     # all 5 .so built
scripts/check_layouts.sh         # LAYOUTS.md parity with IDL
anchor test --skip-lint          # twelve kernel tests pass
```

Then run `anchor test --skip-lint` nine more times — all ten invocations must be green. If any one flakes, fix the flake *before* L2. L3/L4 cannot tolerate a flaky kernel test suite underneath them.

---

## 3. L2 work items — what to build

Per `build_order_part4.md` §4.4.1 and `integration_architecture.md` §2.4, 2.6, 3.1-3.5, 3.7.

### 3.1 `halcyon_oracles` — Pyth read surface

Starting state: empty. Grow into the seam 3.1 uniform interface:

```rust
pub struct PriceSnapshot {
    pub price_s6: i64,         // SCALE_6 signed
    pub conf_s6: i64,
    pub publish_slot: u64,
    pub publish_ts: i64,
    pub expo: i32,
}

pub fn read_pyth_price(
    feed: &AccountInfo,
    now: i64,
    staleness_cap_secs: i64,
) -> Result<PriceSnapshot>;
```

- Feature-flag `pyth-pull` as default. Import `pyth-solana-receiver-sdk` (verify the exact crate name at L2 entry — pin to the version that tracks Pyth pull-model on mainnet as of mid-2026).
- `staleness_cap_secs` is per-call (quote vs settle differ per `ProtocolConfig`).
- Conversion to `i64` SCALE_6 happens in this crate only. Product handlers never see raw Pyth bytes.
- Add `fixtures/` with recorded Pyth price accounts and a `mock-pyth` feature gate for localnet tests; the real feature toggle is `pyth-pull`.

### 3.2 `halcyon_sol_autocall` — the product program

Four public instructions per §4.4.1:

**`preview_quote`** — read-only via `simulateTransaction`.
- Accounts: `protocol_config`, `vault_sigma` (for this product), `regime_signal` (for this product — respected even though SOL Autocall's primary driver is `VaultSigma`, the product may apply a regime multiplier per Part 2.3's list; if the product doesn't use regime, skip this account and document that choice), Pyth SOL price feed, `Clock`.
- Handler:
  1. Freshness: `VaultSigma` via `protocol_config.sigma_staleness_cap_secs`, Pyth via `protocol_config.pyth_quote_staleness_cap_secs`.
  2. `compose_pricing_sigma(vault_sigma, regime_signal?, sigma_floor)` in product-local code (factor it into `halcyon_sol_autocall_quote` if you want it shared; do not put Solana code in the quote crate).
  3. Call `solve_fair_coupon_at_vol(sigma_s6)` from `halcyon_sol_autocall_quote`.
  4. Apply 75% quote share and 50 bps issuer margin. Constants live in `ProtocolConfig` (not hard-coded) so governance can retune.
  5. Return `{premium: u64, max_liability: u64, quote_slot: u64, engine_version: u16}`.
- CU budget: benchmark at layer close, write to `programs/halcyon_sol_autocall/CU_BUDGET.md`.

**`accept_quote`** — full issuance path per seam 3.2.
- Accounts: preview's accounts + `buyer` (Signer, mut), `buyer_usdc`, `vault_usdc`, `treasury_usdc`, `vault_authority`, `vault_state`, `fee_ledger`, `product_registry_entry`, `policy_header`, `product_terms` (init here, product-owned), `coupon_vault` (if you choose to keep it for v1 — see §2.13 tension), `hedge_sleeve` (required — SOL Autocall is hedged), `product_authority` (PDA Signer).
- Handler:
  1. Recompute premium + max_liability inline (same code path as `preview_quote`, factored into a helper).
  2. Slippage gate: `premium ≤ max_premium`, `max_liability ≥ min_max_liability`. `HalcyonError::SlippageExceeded`.
  3. **Richardson confidence gate.** Call `price_autocall_v2_parity` with `N1=10, N2=15` and `price_autocall_v2` at N=64. If `PriceConfidence::Low`, abort with a new product-level error — do *not* fall back to a possibly-mispriced quote. This is the feature that 4.4.1 explicitly calls out: "if confidence is low, the instruction aborts with a specific error rather than selling the user a policy at a price that may be wrong".
  4. Compute `terms_hash` over every input fed to the pricer (entry price, sigma, regime snapshot, product parameters, `engine_version`).
  5. CPI `kernel::reserve_and_issue` signed by `product_authority` PDA.
  6. Populate `ProductTerms` (entry price s6, barriers, schedule, lockout flag, observation index=0, quote-share + margin snapshots, accumulated coupon=0, status=Active).
  7. CPI `kernel::finalize_policy`.
  8. Emit `PolicyIssued` (reuse from `halcyon_common::events`). The kernel already emits one at `reserve_and_issue`; decide now whether the product emits a second with product-specific fields or the single kernel event suffices. Prefer: let the kernel event be canonical and add a product-only `SolAutocallTermsWritten` event.

**`record_observation`** — keeper-triggered every 2 days.
- Accounts: keeper authority (Signer), `keeper_registry`, `policy_header` (mut), `product_terms` (mut), Pyth SOL, `protocol_config`, `vault_state`, `coupon_vault` (mut if coupon paid), `vault_usdc`/`vault_authority`/`buyer_usdc` (for coupon payout), `hedge_sleeve` (for KI), kernel program for CPI on autocall/KI.
- Handler:
  1. Freshness (sigma, Pyth).
  2. Check on-chain clock vs scheduled observation time. Reject if too early.
  3. Read current SOL price, compute ratio vs entry.
  4. If `obs_index < no_autocall_first_n_obs (=1)`, skip autocall check.
  5. Autocall check (ratio ≥ 102.5%): on hit, CPI `apply_settlement` with autocalled payout (entry notional + accumulated + current coupon). Set `PolicyHeader.status = AutoCalled` then `Settled`.
  6. KI check (ratio ≤ 70% intraday in the SOL Autocall design, at observation in the on-chain simplification at v1): on hit, flag KI in `ProductTerms`. Settlement at maturity then uses recovery formula.
  7. Coupon accrual: if above coupon barrier, pay coupon (via `coupon_vault` if you've kept it, else directly via `vault_usdc`).
  8. Increment `current_observation_index`.
  9. Emit `ObservationRecorded` or specialised event per outcome (`PolicyAutoCalled` on autocall).
- Idempotency per seam 3.7: a second call for the same obs_index is a no-op.

**`settle`** — maturity settlement if no autocall fired.
- Accounts: similar to `record_observation` minus `coupon_vault`-specific ones.
- Handler: compute final payout per product rules:
  - No KI ever: notional + any stored coupons.
  - KI breached: recovery = `notional × min(ratio_at_expiry, 1.0)`.
  CPI `apply_settlement`. Mark `status = Settled`.
- Callable by anyone once `now ≥ expiry_ts` (idempotent).

**Wire-up work this implies in the kernel.** Register the product via the kernel admin path you built at L1 (`register_product`) — expected_authority = SOL Autocall's `product_authority` PDA, per-policy and global risk caps, `init_terms_discriminator`. Stored once at L2 bring-up; not a per-test operation.

### 3.3 Keeper registry bring-up

L1 left `KeeperRegistry` at all-zeros. L2 rotates in real authorities for **observation** and **hedge**:

- Admin calls `kernel::rotate_keeper(role=0, new_authority=<observation keypair>)`.
- Admin calls `kernel::rotate_keeper(role=3, new_authority=<hedge keypair>)`.

Keypairs live out-of-tree (ops secrets). The test harness and CLI use dev-generated keypairs. Production keys live in a secrets manager you set up as part of L5 mainnet ceremony.

### 3.4 Observation keeper — `keepers/observation_keeper/`

Per seam 3.7.

- Language: Rust (cheap to reuse `halcyon_sol_autocall_quote` for reads).
- Runtime: long-lived binary, systemd-service-shaped but runnable bare on devnet.
- Architecture:
  1. Load config (RPC endpoint, keypair path, product ID filter).
  2. Poll or subscribe to `PolicyHeader` accounts with `product_program_id == halcyon_sol_autocall::ID AND status == Active`. Prefer `getProgramAccounts` + Anchor account filters; websocket-based subscriptions are a post-v1 nice-to-have.
  3. For each active note, compute the next scheduled observation time from its `ProductTerms.observation_schedule`. Wake on timer.
  4. On fire, read latest Pyth SOL, build `record_observation` instruction, sign with keeper keypair, submit.
  5. Idempotent: if `current_observation_index` has already advanced past the target, skip.
  6. Structured JSON logging to stdout (`serde_json`).
  7. Failure handling: retry on RPC error with exponential backoff, capped at 1 minute; give up and alert after 5 consecutive failures.

**Do not** make this keeper product-general yet. SOL-Autocall-specific logic lives here; L3 adds an IL-Protection-specific observation/settle path through the same binary's dispatch layer.

### 3.5 Hedge keeper — `keepers/hedge_keeper/`

Per seam 3.5 and §2.6.

- Language: Rust; WASM-bind the quote crate if you end up wanting to share with the frontend, but at L2 native Rust is fine.
- Architecture:
  1. Read all active SOL Autocall `PolicyHeader`s.
  2. For each, reprice via `price_autocall_v2_parity` at current Pyth SOL and current `VaultSigma`. Extract delta surface. Interpolate to spot. **Skip on low confidence** — log it, don't rebalance on a mispriced policy.
  3. Apply `hedge_controller::compute_hedge_target` with the `delta_obs_050` policy config. Cap at 75% delta. Band 10%. Minimum trade 1%.
  4. Sum per-note → aggregate SOL target.
  5. Compare against `HedgeBookState.legs[0].current_position_raw` (the kernel's hedge-book structure already supports this — see L1's `HedgeBookState.legs`). Compute the delta to execute.
  6. On-chain 4-day cooldown gate: skip if within cooldown (cooldown enforcement is a post-L2 kernel addition — at L2, enforce it only in the keeper).
  7. Jupiter quote with 50 bps slippage, sanity-check executed price against Pyth (within 100 bps of spot). Reject if divergent.
  8. Execute Jupiter swap as the hedge keeper authority.
  9. CPI `kernel::record_hedge_trade` with the new position, asset_tag (e.g. `b"SOL"`), leg_index=0, trade delta, executed price, execution cost.

Cadence: on each `ObservationRecorded` event (poll or subscribe), plus a 5-day watchdog wake. Manual-wake operator knob for L2-scale testing.

### 3.6 CLI — `tools/halcyon_cli/`

Per §4.4.1. A small Rust binary (`clap` + `anchor-client`) with these subcommands:

- `init-protocol` — one-shot admin bring-up (idempotent; skip if `ProtocolConfig` exists).
- `register-sol-autocall` — admin flow for registering the product.
- `senior-deposit <amount>` — user flow.
- `seed-junior <amount>` — admin flow.
- `preview <notional>` — invokes `preview_quote` via simulateTransaction; prints the quote.
- `buy <notional> --slippage-bps 50` — invokes `accept_quote`.
- `settle <policy>` — manually trigger settle (keeper's job normally, useful for ops).
- `keepers fire-observation <policy>` — force the observation keeper's single-shot path (useful for devnet testing).
- `keepers fire-hedge` — force one hedge pass.
- `status` — dumps all active policies, `VaultState`, `HedgeBookState`, `FeeLedger`.

No frontend. Operator == user for L2.

### 3.7 Devnet deploy

- Verify Pyth SOL/USD feed presence and publish cadence on devnet. If unavailable or flaky, fall back to `research/devnet_mocks/pyth-mock/` (see §4.10 of the build-order for the mock-Pyth contingency).
- Deploy all five programs (kernel, stub_product, flagship_autocall, il_protection, sol_autocall). Stub is deployed but not used — consider gating issuance in the registry if the stub is still at the original program ID. Better: **relocate the stub** to `research/` at L2 start so it's not part of the deploy.
- Airdrop USDC-Dev via the usdc-dev faucet or create a devnet USDC mint the CLI manages.
- Keeper instances run from a VPS or a dev machine; logs ship to wherever your monitoring lands (L2 is pre-Grafana; structured stdout is fine).

### 3.8 End-to-end devnet test

Per §4.4.1. Scripted, unattended, over a real 16-day window.

1. Operator (CLI): `init-protocol` + `register-sol-autocall`.
2. Operator: `senior-deposit 10000` (10k USDC-Dev).
3. Operator: `seed-junior 1000` (1k USDC-Dev).
4. Operator: `buy 500` (500 USDC-Dev notional, 16-day tenor).
5. Every 2 days the observation keeper fires (on its own cadence).
6. Some SOL price movement triggers either autocall or runs to maturity.
7. Hedge keeper fires at least twice across the 16-day window.
8. Final state: `PolicyHeader.status = Settled`, `HedgeBookState` reflects every trade, `FeeLedger.treasury_balance` reflects the treasury fee accrued from the premium split.

The test passing against live Pyth, live Raydium (via Jupiter), and two weeks of wall clock is the gate. Simulated-clock testing is necessary but not sufficient.

### 3.9 Updates to L1 artefacts

- `LAYOUTS.md`: no kernel account changes are expected. If L2 adds any fields (e.g. product-specific flags on `HedgeBookState`), update it and re-run `make layouts-check`.
- Localnet kernel tests: keep the twelve green. Add product-level tests for SOL Autocall's four instructions under `tests/sol_autocall/`. Happy path + at least one failure path per instruction (low-confidence gate, slippage, capacity, paused, Pyth-stale). These run alongside the kernel suite.
- `LEARNED.md`: append L2 findings. Especially: Pyth pull-model quirks, any Anchor bugs encountered at new account shapes, Jupiter routing anomalies, whatever else bites.

---

## 4. What L2 does NOT build

Straight from §4.4.2:

- No IL Protection. No flagship. No regression keeper. No delta keeper with Merkle commitments.
- No frontend.
- No public junior-deposit flow — still admin-only.
- No multi-product observation keeper. L2's observation keeper is SOL-Autocall-specific; L3 generalises the dispatch layer.

Things that are tempting and should wait:

- Don't start the flagship pricer integration "to save time". The flagship has three correction tables and external-counterparty dependencies. It belongs in L4.
- Don't wire a frontend during L2. Frontend against an unstable product IDL gets rewritten in L5 anyway.
- Don't unify SOL Autocall's observation path with "a generic observation framework". The generality discovery happens in L3, not speculatively.
- Don't build a mainnet multisig. That's L5 ceremony.

---

## 5. L2 exit criterion (from §4.4.3)

- A full SOL Autocall lifecycle completes on devnet, unattended, over a real 16-day period, against live Pyth and live Raydium (or mock Pyth per §4.10 if devnet Pyth is unusable).
- Operator issues via CLI; every subsequent state transition happens by keeper. No manual interventions.
- Terminal state is consistent: `PolicyHeader.status = Settled`, `HedgeBookState` reflects all trades, `VaultState` accounting balances, `FeeLedger` shows the expected treasury fee.
- CU benchmarks for `accept_quote`, `record_observation`, `settle` documented in `programs/halcyon_sol_autocall/CU_BUDGET.md` and under the 1.4M CU envelope with headroom.
- L1's twelve kernel tests still pass (10/10 runs).
- `LEARNED.md` appended with any new gotchas.

If any of these fails, L3 doesn't start. There is no "almost".

---

## 6. Practical suggestions

- **Apply the L1 CPI pattern from day one.** Every SOL Autocall handler that CPIs the kernel must pass kernel-owned PDAs without `seeds + bump` constraints. `Account<T>` validation is sufficient. Same applies to IL and Flagship when their turns come.
- **Commit at every exit gate.** A half-wired keeper between commits is a bisect hazard in L3.
- **Track CU budgets early.** The Richardson N2=15 path is the CU-heaviest branch of `accept_quote`. If it exceeds 400K CU document the breakdown and consider whether to tighten the convergence threshold.
- **Use `/Users/dominic/.claude/projects/-Users-dominic-colosseumfinal/memory/` for durable context.** Save user memories as Dom reveals preferences; save project memories when the SOL Autocall economics tighten or loosen (pricing parameter choices matter for L3 and L4's risk-cap tuning).
- **Dom is a math teacher solo-building this.** He is fluent in the pricing math and backtests; his product/UX instincts are weaker. CLI surface choices are fine; frontend-shaped questions should defer to L5. Pitch explanations at a quant level, not a "Solana veteran" level.
- **If Pyth pull-model is a mess on devnet**, don't burn two weeks on it — shift to the `research/devnet_mocks/pyth-mock/` contingency per build-order §4.10. Preserving the keeper lifecycle surface matters more than pricing-feed fidelity at L2; you'll retest against live feeds as part of L5 mainnet prep.
- **Use `simplify` (skill) on each of the four product instructions once they compile.** The pattern repeats three times in L3/L4 so collapsing boilerplate early pays off.
- Dom's GitHub is `DJB8787`. Branch off `main` for L2 work; open one PR per exit gate rather than one mega-PR.

---

*Handoff complete. Start with §3.1 (`halcyon_oracles`). Read the context documents (§1) before touching any code.*
