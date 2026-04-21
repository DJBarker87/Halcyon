# Halcyon — LEARNED notes

Hard-won facts from integration work. Consulted by subsequent layers before
they write their equivalents.

---

## L1 — Kernel

### 1. Anchor 0.32.1 seed-constraint aliasing bug on kernel-owned PDAs passed through a product → kernel CPI

**Symptom.** With kernel's `ReserveAndIssue` / `ApplySettlement` / `FinalizePolicy`
declaring account constraints like:

```rust
#[account(mut, seeds = [seeds::VAULT_STATE], bump)]
pub vault_state: Account<'info, VaultState>,
```

and the stub product invoking kernel via CPI, kernel's deserialized
`vault_state.total_senior` came out as **zero**, even though the on-chain data
(verified via an RPC `getAccountInfo` immediately before the CPI) contained
the correct non-zero value. The `version: u8` field at offset 0 of the struct
deserialized correctly; every subsequent field read as zero. Follow-up reads
via `ctx.accounts.vault_state.to_account_info()` then access-violated at
address `0x0` / `0x8` of size 8.

**Reproduction.** Single-program localnet, `solana-cli 2.3.0`,
`anchor-cli 0.32.1`, SBF `rustc 1.84`. Happy-path issuance where stub CPIs
`reserve_and_issue` after the vault has a non-zero senior deposit.
`cargo tree` shows a single `anchor-lang = 0.32.1` in the graph.

**Root cause (hypothesis).** Anchor's generated seed-validation for
`Account<'info, T>` appears to leave the cached `T` or the backing
`AccountInfo` in a corrupted state when the handler is reached via CPI. The
pattern is consistent with a memory aliasing issue triggered by interaction
between the seed constraint's PDA derivation and the CPI input memory layout.
Direct single-program calls (e.g. `deposit_senior`) with the same
`seeds = [SEED], bump` on the same `VaultState` account are unaffected.

**Fix applied at L1.** Drop the `seeds + bump` constraint on kernel-owned
PDAs that are passed through a product → kernel CPI. Retain `Account<T>`
discriminator + owner validation — this is sufficient because a product
cannot fabricate an account whose 8-byte discriminator equals
`sha256("account:<KernelType>")[..8]` and whose owner matches `halcyon_kernel`.
Applied to `ReserveAndIssue`, `ApplySettlement`, `FinalizePolicy` in
`programs/halcyon_kernel/src/instructions/lifecycle/`.

Seed-constrained init (creating new PDAs with `init` + `seeds`) is
unaffected and remains in use for `initialize_protocol`, `register_product`,
and the policy header itself.

**Follow-up.** Revisit with Anchor 0.33+ when it lands. If the bug is fixed
upstream, re-add the seed constraints and delete this workaround.

---

### 2. integration_architecture.md §2.10 is stale — Pattern B (§3.2) is canonical

**Symptom.** §2.10 describes the issuance CPI as a kernel→product callback
(kernel calls `product::init_terms` mid-`reserve_and_issue`). §3.2's
"Two CPIs per issuance, not one" pseudocode describes the opposite —
product makes two forward CPIs into the kernel, with the product writing
`ProductTerms` locally in between.

**Resolution (Dom, 2026-04-18).** §3.2 is canonical. §2.10 is an
earlier draft that was never updated. The "re-entrance panic" concern
in §2.10 and in this file's L1 exit criterion §4.3.3 does **not** apply
to the canonical pattern — there is no kernel→product CPI, just two
sequential product→kernel CPIs.

**What this means for the kernel:**

- `ProductRegistryEntry.init_terms_discriminator` is metadata only.
  Nothing invokes it. L2+ products must not design around a callback.
- The canonical flow is:
  1. product → `kernel::reserve_and_issue` (creates `PolicyHeader` in
     `Quoted`, records `terms_hash`, takes premium).
  2. product writes `ProductTerms` locally (no CPI).
  3. product → `kernel::finalize_policy` (rehashes `ProductTerms`
     bytes, compares to `terms_hash`, flips to `Active`).
- Exit-criterion tests for the pattern replace the §4.3.3 re-entrance
  list with these four:
  - (i) Happy path: Quoted → terms written → Active, `product_terms`
    address recorded correctly.
  - (ii) Mid-txn abort after `reserve_and_issue` but before
    `finalize_policy` → atomicity rolls everything back; `PolicyHeader`
    does not persist.
  - (iii) `finalize_policy` passed a `product_terms` account whose
    bytes don't hash to `PolicyHeader.terms_hash` → reject with
    `TermsHashMismatch`.
  - (iv) `finalize_policy` on a header not in `Quoted` → reject with
    `PolicyNotQuoted`.

**Doc TODOs (Dom owns; not blocking L1):**
- Rewrite integration_architecture.md §2.10 steps 3–4 to Pattern B.
- Rewrite build_order_part4.md §4.3.3 to replace the re-entrance
  reference with the four-test list above.

---

## L2 — SOL Autocall

### 1. `terms_hash` must cover the exact `ProductTerms` account bytes, not an ad hoc quote tuple

**Symptom.** `accept_quote` could reserve capital successfully, but
`kernel::finalize_policy` rejected the product terms with
`TermsHashMismatch`.

**Root cause.** The kernel's finalizer re-hashes
`product_terms.try_borrow_data()` and compares that digest to
`PolicyHeader.terms_hash`. That means the product-side hash helper must hash
the exact account image the kernel will see: 8-byte Anchor discriminator plus
borsh-serialized `SolAutocallTerms`. Hashing a hand-picked tuple of quote
inputs is not equivalent once the on-chain terms layout evolves.

**Fix applied at L2.** `programs/halcyon_sol_autocall/src/pricing.rs`
now hashes `SolAutocallTerms::DISCRIMINATOR || borsh(terms)` via
`hash_product_terms`, and `accept_quote` computes `terms_hash` from the fully
populated `SolAutocallTerms` struct before the first kernel CPI.

**Carry-forward rule.** Any product using the same two-CPI issuance pattern
must treat `terms_hash` as an account-bytes commitment, not a business-fields
commitment. If the account shape changes, the hash helper changes with it.

### 2. SOL Autocall coupon cash flows happen on observation dates; terminal settlement must not re-pay historical coupons

**Symptom.** A naive implementation can accidentally pay the buyer twice:
once when a coupon observation occurs and again at autocall / maturity by
settling against the full accumulated coupon counter.

**Resolution at L2.** `record_observation` now pays non-terminal coupons
through a kernel `pay_coupon` CPI at the observation date, while terminal
paths only include the current unpaid coupon plus redemption. `settle`
likewise only includes the final unpaid coupon, not the full coupon history.

**Carry-forward rule.** For autocall products, the money-moving truth is:
coupon observations transfer cash when they happen; final settlement is only
for redemption plus whatever coupon has not yet been transferred.

### 3. The only fully specified production issuance gate in the local SOL Autocall docs is `fair coupon >= 50 bps / observation`

**Docs reviewed.** `docs/halcyon_sol_autocall_v1_spec.md`,
`product_economics/sol_autocall_product_economics_report.md`,
`product_economics/sol_autocall_economics_explainer.md`,
`halcyon_whitepaper_v9.md`.

**Result.** The production rule is concrete: no quote below a fair coupon of
50 bps per observation. A second number appears in the economics report
(`coupon-alive ratio cap = 50% of active notes`) and the explainer says the
system may also stop issuance when "too many existing notes are in trouble",
but neither source defines a formula tight enough to implement on-chain.

**Fix applied at L2.** `solve_quote` now enforces the 50 bps floor. Preview
returns a zeroed quote for no-quote states; issuance aborts with a dedicated
error. The CLI treats zeroed previews as "no quote" rather than trying to buy
them.

**Carry-forward rule.** When docs contain prose about portfolio-health gates
without a precise trigger formula, do not invent one in code. Either get the
formula written down or leave the rule flagged as unresolved.

### 4. L2 is still blocked on real hedge execution and sleeve funding, not on pricing math

**What works now.** The hedge keeper computes the SOL target from the shared
pricing code, respects the observation/issuance wake logic, and records hedge
book state with monotonic sequencing.

**What is still missing.**
- No Jupiter execution adapter exists in-repo yet; the keeper currently stops
  at dry-run logging unless explicitly told to record a synthetic trade.
- `CouponVault` / `HedgeSleeve` state structs exist in the kernel, but the
  separate funded token-account pools described in the architecture docs are
  not wired into registration, funding, or keeper execution.

**Carry-forward rule.** Treat the current hedge path as accounting +
replanning only. L3/L4 should not assume L2 has already solved real DEX
execution or separate sleeve custody.

### 5. SOL Autocall §11 shipping path is keeper-fed `P_red(σ)`, not the live E11 operator assembly

**Symptom.** After eliminating the E11 heap OOM, the fixed-product live
operator path still measured around 1.36M CU on SBF — well above the
`946,421` CU target recorded in the authority log.

**Resolution.** `research/complexity_reduction_log.md §11` is explicit: the
shipping one-transaction POD-DEIM path stores the basis / DEIM machinery as
compile-time constants, but the sigma-dependent reduced operator `P_red(σ)` is
computed off-chain and written to a product PDA by a keeper. The on-chain
quote path only loads `P_red(σ)` and runs the reduced backward pass.

**Carry-forward rule.** For SOL Autocall compute work, do not equate
"compile-time POD-DEIM tables" with "fully compile-time quote path". The basis
is static; the per-sigma reduced operator is not. Any future agent working on
SOL Autocall CU or pricing should read `research/complexity_reduction_log.md`
before proposing architectural changes.
