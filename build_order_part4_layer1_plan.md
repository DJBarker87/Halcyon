# Halcyon — Layer 1 (Kernel) Execution Plan

**Target audience:** a fresh Claude Code instance starting with zero prior context on this repository.
**Prerequisite:** L0 is complete (see "L0 state handoff" below). Do not start L1 until the L0 exit criterion (4.2.3 of `build_order_part4.md`) passes.
**Exit criterion (L1):** the twelve localnet tests enumerated in 4.3.1 of `build_order_part4.md` pass reliably across ten consecutive invocations, the mutual-CPI pattern works end-to-end with a stub product, and `programs/halcyon_kernel/LAYOUTS.md` exists and matches the compiled IDL.

---

## 1. Context you need before you start

Read these in order. Do not skip. Every decision L1 makes is justified against one of them.

1. `halcyon_whitepaper_v9.md` — what the protocol *is*. Three products (flagship SPY/QQQ/IWM worst-of autocall, IL Protection, SOL Autocall) share one vault, three uncorrelated failure modes, backtested vault returns 2.2–5.6% annualised, zero insolvencies.
2. `integration_architecture.md` Parts 1–3 — the target system, the eight seams, the invariants. The kernel surface L1 implements is specified in Part 2.3 (state topology), 2.4 (lifecycle surface), 2.9 (upgrade posture), 2.10 (mutual-CPI pattern), 2.11 (ALTs), 2.13 (named tensions).
3. `build_order_part4.md` §4.3 — the L1 definition itself. Work items, what-is-not-built, exit criterion. Authoritative.
4. `MEMORY.md` in the project memory store — who Dom is, and that the landing page currently pitches the wrong products (landing-page fix is *not* L1 scope but don't build away from it).

When Parts 2, 3, and build-order diverge on a detail, `integration_architecture.md` wins. If it's silent, `build_order_part4.md` wins.

---

## 2. L0 state handoff

What exists in the repo right now (as of L0 exit):

### Workspace layout

```
crates/
  halcyon_common/                 scale constants only — grows in L1 (PDA seeds, error codes, events)
  halcyon_kernel_types/           empty — grows in L1 (account layouts for cross-program reads)
  halcyon_oracles/                empty — grows in L2
  halcyon_flagship_quote/         flagship pricer (17 modules), pure Rust, zero Solana deps
  halcyon_il_quote/               IL pricer (insurance/*), pure Rust; pool/* carried along as reference-only (NOT in lib.rs)
  halcyon_sol_autocall_quote/     SOL Autocall pricer + hedge controller, pure Rust
  halcyon_client_sdk/             empty — grows L2+
  halcyon-wasm/                   browser shim, unchanged from pre-L0 except import paths updated
programs/
  halcyon_kernel/                 empty #[program], declare_id! only
  halcyon_flagship_autocall/      empty #[program], declare_id! only
  halcyon_il_protection/          empty #[program], declare_id! only
  halcyon_sol_autocall/           empty #[program], declare_id! only
keepers/
  observation_keeper/             empty main.rs (println only)
  regression_keeper/              empty main.rs
  delta_keeper/                   empty main.rs
  hedge_keeper/                   empty main.rs
  regime_keeper/                  empty main.rs
solmath-core/                     unchanged (published to crates.io, do not modify)
samples/                          il_hedge_request.json, sol_autocall_request.json (backtest replay inputs)
scripts/localnet.sh               launches solana-test-validator at RPC :8899
.github/workflows/ci.yml          cargo check + cargo test + anchor build + make sol-autocall
Anchor.toml                       pinned anchor 0.32.1 / solana 2.3.0
Makefile                          `make bootstrap`, `make test`, `make sol-autocall`, `make anchor-build`, `make localnet`
```

### Program IDs (already deployed via `declare_id!` and `Anchor.toml`)

- `halcyon_kernel`            = `H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF`
- `halcyon_flagship_autocall` = `E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc`
- `halcyon_il_protection`     = `HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g`
- `halcyon_sol_autocall`      = `6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8`

Keypairs live at `target/deploy/<name>-keypair.json`. Do not regenerate them. Back them up if you wipe `target/`.

### Toolchain pins

- `anchor-cli 0.32.1`, `anchor-lang = "=0.32.1"` everywhere
- `solana-cli 2.3.0`, `cargo-build-sbf 2.3.0`, `platform-tools v1.48`
- host `rustc 1.93`, SBF `rustc 1.84`

### Known L0-era quirks

- `halcyon-wasm`'s `[profile.release]` lives inside the package's Cargo.toml and triggers a cargo warning (`profiles for the non root package will be ignored`). Harmless at L0 because we only use release wasm via `make app-wasm` which invokes cargo directly. **If this bites L1**, move the profile to the workspace root.
- `halcyon_flagship_quote/src/worst_of_factored.rs` line ~2310: one literal was pinned `1.0_f64` during L0 to disambiguate `.clamp` under SBF `rustc 1.84`. If you touch this line, keep the annotation.
- Programs depend on their quote crate with default features (i.e. `full`). Do not add `default-features = false` on those deps unless you explicitly also add a matching feature list — solmath-core's `insurance` feature is required and inherited via `full`.
- `crates/halcyon_il_quote/src/pool/*` is on disk but **not** declared in `src/lib.rs`. It references a `halcyon_common::{fp,fees,constants}` surface that does not exist. Leave it alone in L1.

### Verification before you start L1

Run every one of these. If any fails, fix it *before* writing a line of L1 code:

```
cargo check --workspace --exclude halcyon-wasm
cargo test  -p halcyon_sol_autocall_quote --test smoke
cargo test  -p halcyon_il_quote           --test smoke
make sol-autocall
anchor build
ls target/deploy/halcyon_kernel.so target/deploy/halcyon_flagship_autocall.so \
   target/deploy/halcyon_il_protection.so target/deploy/halcyon_sol_autocall.so
```

All must succeed. `cargo test --workspace` is currently noisy because the quote crates carry ~50 warnings; that's pre-existing debt, not yours.

---

## 3. L1 work items — what to build

Per `build_order_part4.md` §4.3.1 and `integration_architecture.md` §2.3, 2.4, 2.10.

### 3.1 `halcyon_common` — flesh out the shared surface

Starting state: `SCALE_6` and `SCALE_12` constants only. Add:

- **PDA seeds** as `pub const` byte-literals. Every seed used in §3.2 of `integration_architecture.md`:
  `b"protocol_config"`, `b"product_registry"`, `b"vault_state"`, `b"senior"`, `b"junior"`, `b"policy"`, `b"terms"`, `b"coupon_vault"`, `b"hedge_sleeve"`, `b"hedge_book"`, `b"aggregate_delta"`, `b"regression"`, `b"vault_sigma"`, `b"regime_signal"`, `b"fee_ledger"`, `b"keeper_registry"`, `b"alt_registry"`, `b"product_authority"`.
- **Fixed-point helpers**: `to_scale_6(u64) -> i64`, `to_scale_12(u64) -> i128`, overflow-safe conversions. Used at the product handler boundary per seam 3.1's invariant.
- **Error codes** (`HalcyonError` enum, annotated with Anchor's `#[error_code]`). Cover at minimum: `Overflow`, `SigmaStale`, `RegimeStale`, `RegressionStale`, `PythStale`, `PausedGlobally`, `IssuancePausedPerProduct`, `SettlementPausedGlobally`, `ProductAuthoritySignatureMissing`, `ProductAuthorityMismatch`, `ProductPaused`, `CapacityExceeded`, `UtilizationCapExceeded`, `RiskCapExceeded`, `SlippageExceeded`, `PolicyNotQuoted`, `PolicyNotActive`, `CorrectionTableHashMismatch`, `CooldownNotElapsed`, `BelowMinimumTrade`.
- **Event schemas** — one Rust struct per event named in 4.3.1: `PolicyIssued`, `PolicySettled`, `HedgeBookUpdated`, `KeeperRotated`, `ConfigUpdated`, `FeesSwept`. Derive `AnchorSerialize + AnchorDeserialize + anchor_lang::event::Event`. Export from `events.rs` so keepers can `use halcyon_common::events::PolicyIssued;` directly.

This crate will now need `anchor-lang = "=0.32.1"` as a real dep (add it). Drop `#![no_std]` from `lib.rs` — Anchor's derive macros pull in `std`.

### 3.2 `halcyon_kernel_types` — account layouts exported for cross-program reads

Every kernel PDA layout that any *product* program needs to read (not mutate). At L1 the product programs still barely exist, but the types need to exist so L2 onwards can `use halcyon_kernel_types::ProtocolConfig;` in a product handler without circular deps.

Minimum set per §3.1 of `integration_architecture.md`: `ProtocolConfig`, `VaultSigma`, `RegimeSignal`, `Regression`, `ProductRegistryEntry`, `PolicyHeader`, `KeeperRegistry`.

Each type is a `#[derive(AnchorSerialize, AnchorDeserialize, Clone)]` struct with the field layout matching the Anchor `#[account]` in the kernel itself. Keeping the layout in two places is the price of not creating a circular dep — document this explicitly in the crate's `lib.rs` doc comment.

### 3.3 `halcyon_kernel` — the program itself

This is the bulk of L1.

**Anchor `#[account]` structs** for every PDA in `integration_architecture.md` §2.3:

- `ProtocolConfig` (singleton), `ProductRegistryEntry`, `VaultState`, `SeniorDeposit`, `JuniorTranche`, `PolicyHeader`, `CouponVault`, `HedgeSleeve`, `HedgeBookState`, `AggregateDelta`, `Regression`, `VaultSigma`, `RegimeSignal`, `FeeLedger`, `KeeperRegistry`, `LookupTableRegistry`.

Field-by-field layout:

- Every numeric field: `u64` for USDC/token amounts in their natural 6-decimal scale; `i64` for signed fixed-point pricing quantities at SCALE_6; `u128` only for intermediate overflow protection — do not expose `u128` on-disk unless there's a specific reason.
- Every struct has `u8 version` at offset 0 for in-place upgrade migration.
- Every struct has an explicit `pub const SPACE: usize = 8 + ...` constant (the `8` is Anchor's account discriminator).
- Document per-field byte widths in `programs/halcyon_kernel/LAYOUTS.md`. This doc is an L1 deliverable, not a nice-to-have — the exit criterion checks that LAYOUTS.md matches the compiled IDL at layer boundary.

**Instructions** per `integration_architecture.md` §2.3 bullet list, grouped:

- Admin: `initialize_protocol`, `set_protocol_config`, `pause_issuance`, `pause_settlement`, `rotate_keeper`, `register_product`, `update_product_registry`, `register_lookup_table`, `update_lookup_table`.
- Capital: `deposit_senior`, `withdraw_senior` (7-day cooldown), `seed_junior` (admin-only at v1), `sweep_fees`.
- Oracle state writes: `update_ewma` (permissionless, 30s rate limit), `write_regression` (keeper-gated), `write_regime_signal` (keeper-gated), `write_aggregate_delta` (keeper-gated, flagship-only at L4 — handler exists at L1 but acts as a no-op until the flagship product registers).
- Policy lifecycle: `reserve_and_issue`, `finalize_policy`, `apply_settlement`, `record_hedge_trade`.

Every handler follows the pattern in §3.3 of `integration_architecture.md`:

1. Authentication (signer check + registry check for product CPIs; admin multisig check for admin ixs)
2. Global pause check
3. Capacity check (`utilization_cap_bps`, `per_policy_risk_cap`)
4. Token transfer (if any)
5. State mutation
6. Account creation/update

`checked_add` and `checked_mul` everywhere. `u128` intermediates for premium splits.

**Validation order is stable and documented in comments per handler.** This is not a nice-to-have; auditors read the order, and drifting it between handlers masks bugs.

### 3.4 Stub product — `programs/halcyon_stub_product/`

Per §4.3.1 of `build_order_part4.md`:

- Create this as a fifth program. Give it its own keypair (`solana-keygen new` → target/deploy) and `declare_id!`.
- Three instructions: `accept_quote_stub`, `settle_stub`, `init_terms_stub`.
- A trivial `ProductTerms` account with one field: `pub magic: u64`.
- Its sole purpose is to exercise the kernel's mutual-CPI seam at `reserve_and_issue` → (kernel creates `PolicyHeader` in `Quoted`) → product writes `ProductTerms` → `finalize_policy` (kernel flips to `Active`). Keep it deliberately trivial so kernel bugs surface as kernel bugs.
- **Delete or relocate to `research/` at the start of L2.** Leave a TODO at the top of its `lib.rs` that says so.

### 3.5 Mutual-CPI seam — the real integration work

§2.10 of `integration_architecture.md` walks through the five-step pattern. The Anchor hazard called out in 2.10 and 4.3.3 is real: the kernel's `finalize_policy` cannot re-borrow `PolicyHeader` mutably during the CPI because Anchor's account-constraints macro can re-lock. The documented fix is to split the `PolicyHeader` mutation across the two kernel instructions cleanly — `reserve_and_issue` does the initial `set_inner` with status=`Quoted` and `product_terms = Pubkey::default()`; `finalize_policy` does a clean `policy_header.status = Active` + `policy_header.product_terms = <addr>` with the header freshly borrowed.

If you hit a different Anchor gotcha in this seam, **document it in `LEARNED.md` at repo root**. L2-L4 consult this file.

### 3.6 ALT plumbing

`register_lookup_table` / `update_lookup_table` instructions live in the kernel. The localnet script already launches validator; L1 adds at minimum one ALT registration in a test that resolves `protocol_config + product_registry_entry + kernel_program` into a lookup table, and issuance via that ALT. This exercises the v0-transaction path from day one — it's the property that prevents the flagship's 32-account limit from biting in L4.

### 3.7 Tests — twelve localnet integration tests

Per §4.3.1 of `build_order_part4.md`, written against Anchor's TypeScript test harness. Copy the enumerated list verbatim:

1. Fresh protocol initialize → `ProtocolConfig` exists with expected defaults.
2. Register stub product → `ProductRegistryEntry` visible, `product_authority` PDA recorded.
3. Senior deposit → `SeniorDeposit` account created, `VaultState.total_senior` updated.
4. Senior withdraw within cooldown → fails with expected error.
5. Senior withdraw past cooldown → succeeds, `VaultState` updated.
6. Happy-path issuance: stub calls `accept_quote_stub` → kernel CPI `reserve_and_issue` → kernel CPI back into `init_terms_stub` → `PolicyHeader` transitions `Quoted` → `Active` atomically.
7. Issuance while `issuance_paused_global` → fails with `PausedGlobally`.
8. Issuance above capacity → fails with `CapacityExceeded`.
9. Settle happy path: stub calls `settle_stub` → kernel CPI `apply_settlement` → buyer receives clamped payout, unused reservation returned to free capital, `PolicyHeader.status = Settled`.
10. Settle while `settlement_paused_global` → fails.
11. Replay of the same settle call → fails (PolicyHeader not in Active state).
12. ALT-based v0 transaction: issuance path via v0 with lookup table resolving kernel + config + registry entry → succeeds.

Test harness skeleton goes in `tests/kernel/` (Anchor's default `tests/` directory). Use `anchor test --skip-lint` at CI. Update `.github/workflows/ci.yml` to actually run `anchor test` rather than today's `|| echo` skip.

The bar is not test coverage for its own sake — it's having tests that L2/L3/L4 can grep for when they break something in the kernel.

### 3.8 Events

Every kernel instruction that mutates money or status emits one of the events from 3.1 above. Emit via `emit!()`. Keepers subscribe to these starting in L2, so the wire-format stability matters — version-bump the event structs rather than silently extending them.

### 3.9 `LAYOUTS.md` and the IDL-parity check

`programs/halcyon_kernel/LAYOUTS.md` is authored by hand. After `anchor build`, a small script (add to `Makefile` as `make layouts-check`) parses `target/idl/halcyon_kernel.json` and asserts each account's field order and sizes match what LAYOUTS.md claims. Keepers and the frontend build against the IDL; humans reason against LAYOUTS. Drift between the two has a real integration cost that shows up in L4 or L5.

---

## 4. What L1 does NOT build

Straight from §4.3.2:

- No real product handlers (besides the stub).
- No pricer integration — the stub calls no quote crate.
- No keeper authorities beyond the stub's observation path.
- No frontend.
- No devnet or mainnet deploy.
- No `deposit_junior` public instruction — v1 is founder-seeded via `seed_junior` which is admin-only. Post-v1 work.

Things that are tempting and should wait:

- Don't try to flesh out `halcyon_oracles` while writing the kernel. Oracles are L2 — the kernel doesn't read Pyth directly anywhere.
- Don't start writing the flagship, IL, or SOL Autocall product handlers "to save time". Those are L2/L3/L4 and depend on the kernel surface being frozen. A half-built product program during L1 is how kernel bugs and product bugs get tangled together.
- Don't write keeper logic. Five empty `main.rs` stubs stay empty until L2.

---

## 5. L1 exit criterion (from §4.3.3)

- Twelve localnet tests pass reliably across ten consecutive invocations.
- The mutual-CPI pattern works and any Anchor account-constraints gotcha encountered has a documented fix in `LEARNED.md`.
- `programs/halcyon_kernel/LAYOUTS.md` exists and matches the compiled IDL via `make layouts-check`.
- `cargo test --workspace --exclude halcyon-wasm` passes.
- `anchor build` produces five `.so` files (kernel + four product scaffolds + stub).
- `make bootstrap` still works — L0's contract is preserved.

If one of these fails, L2 doesn't start. There is no "almost".

---

## 6. Practical suggestions

- **Commit at every exit gate**, not every file. A kernel instruction half-written between commits is a bisect hazard in L2.
- **When Anchor trips you, search issues on `coral-xyz/anchor` at v0.32.1 first.** The macro surface has footguns; most are known.
- **If the CI anchor-build job is slow**, cache `~/.cache/solana` and `target/` aggressively. The build is CPU-heavy but cacheable.
- **Track CU budgets early.** Kernel instructions should all clock well under 200K CU each — if any single handler exceeds that, the design is probably wrong. Document actual CU in comments once `anchor test` surfaces them.
- **Use `/Users/dominic/.claude/projects/-Users-dominic-colosseumfinal/memory/` for durable context.** Save feedback from Dom as you go; Dom is solo-building, he's the single source of product truth. He's a math teacher — pitch design questions at his level of quantitative fluency, not at "Solana veteran" fluency.
- Dom's GitHub is `DJB8787`. Branch off `main` for L1 work; open one PR per exit gate rather than one mega-PR.

---

*Handoff complete. Start with §3.1 (`halcyon_common`). Read the three context documents (§1) before touching any code.*
