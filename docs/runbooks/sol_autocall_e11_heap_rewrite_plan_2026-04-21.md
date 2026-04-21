# SOL Autocall E11 Heap-Elimination Rewrite Plan

## Goal

Eliminate heap allocation from the fixed-product on-chain E11/POD-DEIM runtime path used by `preview_quote` and `accept_quote`.

Current failure mode on devnet:

- `preview_quote` now dispatches to E11 first as intended.
- The program aborts before `generated_quote_context()` returns with `memory allocation failed, out of memory`.
- `generated_quote_context()` currently materializes about 80 KB of `Vec` payload from generated const tables, which is incompatible with the default SBF heap.
- Transaction-side `request_heap_frame(256 KB)` did not unblock the failure.
- Program-side `custom-heap` currently builds but does not deploy (`sol_alloc_free_` unresolved), so it is not a viable near-term workaround.

This document scopes the rewrite only. No solver behavior changes are proposed here.

## Clarifications Confirmed Before Execution

- `EIM_ROWS` and `EIM_COLS` in the generated table already use `u16` in
  [pod_deim_table.rs](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/generated/pod_deim_table.rs:73).
  No codegen regeneration step is needed for Group 1.
- The original “try `[i128; 225]` first” branch for `p_hp` is rejected up front.
  `225 * 16 = 3600` bytes leaves under `500` bytes of theoretical frame headroom
  before any existing locals in the same BPF frame, so the implementation should
  use scalar or row-scratch accumulation instead of a large `i128` stack array.
- Deployment must happen only after an off-chain parity checkpoint. The
  representative sigma sweep is:
  `0.80, 1.00, 1.17, 1.50, 2.00`.
  The refactored const path must match the pre-refactor reference to within the
  SCALE_6 quantization unit before devnet measurement.

## Non-goals

- Do not change the pricing model, E11 gates, or Richardson fallback semantics.
- Do not change the host-side training/generation pipeline except for compile-fix fallout.
- Do not introduce a permanent larger-heap dependency.

## Proposed Design

Use separate zero-allocation runtime view types for the fixed-table path instead of forcing lifetimes through the host-side training structs.

Proposed runtime-only types:

- `E11QuoteContextConst<'a>`
- `E11FactorsConst<'a>`
- `DeimFactorsConst<'a>`
- `DeimLegConst<'a>`
- `MarkovGridInfoConst<'a>`

Design rules:

- Read-only generated tables become `&'static [T]` or `&'static [bool]`.
- EIM row/col indices stay in generated representation as `&'static [u16]`; cast to `usize` at the use site instead of allocating `Vec<usize>`.
- Mutable scratch becomes fixed-size stack buffers when the frame is comfortably below the 4 KB BPF limit.
- Large fixed scratch that risks the BPF frame limit is marked `special-case` and should be implemented with either:
  - a helper-local fixed array if frame size stays safe, or
  - a smaller row-scratch / caller-provided scratch scheme.
- Remove the `clone_deim_leg` patching step entirely from the runtime path. Pass `p_red_v` and `p_red_u` directly to the DEIM solver alongside read-only leg references.

## Allocation Inventory And Rewrite Targets

### 1. `generated_quote_context()` owner

Location: [autocall_v2_e11.rs](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:317)

Refactor intent:

- Replace this function with `generated_quote_context_const() -> E11QuoteContextConst<'static>`.
- It should return only references into [pod_deim_table.rs](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/generated/pod_deim_table.rs:1).
- Zero allocation target: this function should not allocate at all.

Sites:

- `factors.atoms_v` at [autocall_v2_e11.rs:322](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:322)
  Current: `Vec<i64>` via `generated::ATOMS_V.to_vec()` length `M*D*D = 2700`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `E11Factors -> E11FactorsConst`, `solve_fair_coupon_e11`

- `factors.atoms_u` at [autocall_v2_e11.rs:323](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:323)
  Current: `Vec<i64>` via `generated::ATOMS_U.to_vec()` length `2700`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `E11Factors -> E11FactorsConst`, `solve_fair_coupon_e11`

- `factors.p_ref_red_v` at [autocall_v2_e11.rs:324](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:324)
  Current: `Vec<i64>` via `to_vec()` length `D*D = 225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_e11`

- `factors.p_ref_red_u` at [autocall_v2_e11.rs:325](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:325)
  Current: `Vec<i64>` via `to_vec()` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_e11`

- `factors.p_ref_at_eim` at [autocall_v2_e11.rs:326](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:326)
  Current: `Vec<i64>` via `to_vec()` length `M = 12`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_e11`

- `factors.b_inv` at [autocall_v2_e11.rs:327](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:327)
  Current: `Vec<i64>` via `to_vec()` length `M*M = 144`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_e11`

- `factors.eim_rows` at [autocall_v2_e11.rs:328](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:328)
  Current: `Vec<usize>` via `collect()`, length `M = 12`
  Proposed: `&'static [u16]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_e11` index reads cast with `as usize`

- `factors.eim_cols` at [autocall_v2_e11.rs:329](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:329)
  Current: `Vec<usize>` via `collect()`, length `M = 12`
  Proposed: `&'static [u16]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_e11`

- `factors.grid_reps` at [autocall_v2_e11.rs:330](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:330)
  Current: `Vec<i64>` via `to_vec()` length `N = 50`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_e11`

- `factors.grid_bounds` at [autocall_v2_e11.rs:331](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:331)
  Current: `Vec<i64>` via `to_vec()` length `N-1 = 49`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_e11`

- `deim_base.v_leg.p_red` at [autocall_v2_e11.rs:335](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:335)
  Current: `Vec<i64>` zero-initialized length `D*D = 225`
  Proposed: remove from const view entirely
  Classification: `special-case`
  Downstream updates: `DeimLegData -> DeimLegConst`, `solve_fair_coupon_e11`, `solve_fair_coupon_deim`
  Note: this placeholder is dead in the const path because runtime `p_red` is assembled live.

- `deim_base.v_leg.phi_at_idx` at [autocall_v2_e11.rs:336](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:336)
  Current: `Vec<i64>` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`, `deim_matvec6`

- `deim_base.v_leg.pt_inv` at [autocall_v2_e11.rs:337](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:337)
  Current: `Vec<i64>` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`, `deim_matvec6`

- `deim_base.v_leg.phi_atm` at [autocall_v2_e11.rs:338](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:338)
  Current: `Vec<i64>` length `D = 15`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.v_leg.m_ki_red` at [autocall_v2_e11.rs:339](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:339)
  Current: `Vec<i64>` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `DeimLegConst`

- `deim_base.v_leg.m_nki_red` at [autocall_v2_e11.rs:340](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:340)
  Current: `Vec<i64>` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `DeimLegConst`

- `deim_base.v_leg.ki_at_idx` at [autocall_v2_e11.rs:341](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:341)
  Current: `Vec<bool>` length `15`
  Proposed: `&'static [bool]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.v_leg.cpn_at_idx` at [autocall_v2_e11.rs:342](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:342)
  Current: `Vec<bool>` length `15`
  Proposed: `&'static [bool]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.v_leg.ac_at_idx` at [autocall_v2_e11.rs:343](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:343)
  Current: `Vec<bool>` length `15`
  Proposed: `&'static [bool]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.v_leg.phi` at [autocall_v2_e11.rs:344](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:344)
  Current: `Vec<i64>` length `N*D = 750`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `phi_transpose_times_v`, `solve_fair_coupon_deim`

- `deim_base.u_leg.p_red` at [autocall_v2_e11.rs:348](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:348)
  Current: `Vec<i64>` zero-initialized length `225`
  Proposed: remove from const view entirely
  Classification: `special-case`
  Downstream updates: same as V-leg `p_red`

- `deim_base.u_leg.phi_at_idx` at [autocall_v2_e11.rs:349](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:349)
  Current: `Vec<i64>` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.u_leg.pt_inv` at [autocall_v2_e11.rs:350](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:350)
  Current: `Vec<i64>` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.u_leg.phi_atm` at [autocall_v2_e11.rs:351](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:351)
  Current: `Vec<i64>` length `15`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.u_leg.m_ki_red` at [autocall_v2_e11.rs:352](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:352)
  Current: `Vec<i64>` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `DeimLegConst`

- `deim_base.u_leg.m_nki_red` at [autocall_v2_e11.rs:353](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:353)
  Current: `Vec<i64>` length `225`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `DeimLegConst`

- `deim_base.u_leg.ki_at_idx` at [autocall_v2_e11.rs:354](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:354)
  Current: `Vec<bool>` length `15`
  Proposed: `&'static [bool]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.u_leg.cpn_at_idx` at [autocall_v2_e11.rs:355](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:355)
  Current: `Vec<bool>` length `15`
  Proposed: `&'static [bool]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.u_leg.ac_at_idx` at [autocall_v2_e11.rs:356](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:356)
  Current: `Vec<bool>` length `15`
  Proposed: `&'static [bool]`
  Classification: `const-table-reference`
  Downstream updates: `solve_fair_coupon_deim`

- `deim_base.u_leg.phi` at [autocall_v2_e11.rs:357](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:357)
  Current: `Vec<i64>` length `750`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `phi_transpose_times_v`, `solve_fair_coupon_deim`

- `grid_info.reps` at [autocall_v2_e11.rs:364](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:364)
  Current: `Vec<i64>` via duplicate `to_vec()` length `50`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `MarkovGridInfo -> MarkovGridInfoConst`, `solve_fair_coupon_deim`

- `grid_info.bounds` at [autocall_v2_e11.rs:365](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs:365)
  Current: `Vec<i64>` via duplicate `to_vec()` length `49`
  Proposed: `&'static [i64]`
  Classification: `const-table-reference`
  Downstream updates: `MarkovGridInfo -> MarkovGridInfoConst`

### 2. `solve_fair_coupon_e11()` owner

Location: [autocall_v2.rs](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2826)

Refactor intent:

- Change solver input from heap-owning `&E11Factors` / `&MarkovGridInfo` / `&DeimFactors` to const views.
- Keep runtime mutable state in fixed-size scratch.
- Remove `clone_deim_leg` and the heap-built `patched` struct entirely.

Sites:

- `dp_vals` at [autocall_v2.rs:2846](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2846)
  Current: `Vec<i64>` length `m`
  Proposed: `[i64; generated::M]`
  Classification: `stack-buffer`
  Downstream updates: none outside `solve_fair_coupon_e11`

- `c_coeffs_hp` at [autocall_v2.rs:2890](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2890)
  Current: `Vec<i128>` length `m`
  Proposed: `[i128; generated::M]`
  Classification: `stack-buffer`
  Downstream updates: `assemble_p_red` helper signature

- `assemble_p_red` return type at [autocall_v2.rs:2903](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2903)
  Current: `Result<Vec<i64>, SolMathError>`
  Proposed: `Result<[i64; generated::D * generated::D], SolMathError>` or `Result<(), SolMathError>` with caller-provided out buffer
  Classification: `special-case`
  Downstream updates: `solve_fair_coupon_e11`, removal of `patched` heap object

- `p_hp` at [autocall_v2.rs:2906](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2906)
  Current: `Vec<i128>` length `d*d = 225`
  Proposed: scalar or row-scratch accumulation with no large `i128` stack array
  Classification: `special-case`
  Downstream updates: only `assemble_p_red`
  Note: this is the only fixed-size runtime scratch that would push too close to the 4 KB BPF frame limit if translated literally.

- `p_red` at [autocall_v2.rs:2920](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2920)
  Current: `Vec<i64>` length `225`
  Proposed: `[i64; generated::D * generated::D]`
  Classification: `stack-buffer`
  Downstream updates: `solve_fair_coupon_deim` should take `p_red_v` / `p_red_u` by reference instead of embedded in cloned legs

- `patched` / `clone_deim_leg` path at [autocall_v2.rs:2932](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2932)
  Current: build a new heap-owning `DeimFactors` by cloning every `Vec` field of both legs, then overwriting `p_red`
  Proposed: remove this object construction entirely; call a zero-allocation DEIM solver that accepts:
  `grid_info`, `v_leg_ref`, `u_leg_ref`, `&p_red_v`, `&p_red_u`, `contract`
  Classification: `special-case`
  Downstream updates: `solve_fair_coupon_deim`, deletion of `clone_deim_leg`

### 3. `clone_deim_leg()` owner

Location: [autocall_v2.rs](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2951)

Refactor intent:

- Delete from the runtime path.
- If host-side code still needs a clone helper, keep a host-only version behind `#[cfg(not(target_os = "solana"))]`.

Sites:

- `p_red` clone at [autocall_v2.rs:2953](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2953)
  Current: `leg.p_red.clone()`
  Proposed: no clone, no helper
  Classification: `special-case`
  Downstream updates: removed with `patched`
  Note: this clone is currently wasted because the struct update overwrites it immediately.

- `phi_at_idx`, `pt_inv`, `phi_atm`, `m_ki_red`, `m_nki_red`, `ki_at_idx`, `cpn_at_idx`, `ac_at_idx`, `phi` clones at [autocall_v2.rs:2954](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2954) through [2962](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2962)
  Current: `Vec` clones of read-only leg data
  Proposed: no clone, pass `DeimLegConst<'_>` references through
  Classification: `const-table-reference`
  Downstream updates: removed with `patched`

### 4. `nig_cdf_cos_at()` owner

Location: [autocall_v2.rs](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2971)

Site:

- `coeffs` at [autocall_v2.rs:2995](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2995)
  Current: `Vec<(i64, i64)>` via `Vec::new()` + `push()`, dynamic length with compile-time upper bound `COS_M - 1 = 16`
  Proposed: `[(i64, i64); COS_M - 1]` plus `coeffs_len: usize`
  Classification: `special-case`
  Downstream updates: `nig_cdf_cos_direct` should accept `&[(i64, i64)]`, so pass `&coeffs[..coeffs_len]`
  Alternative: `ArrayVec<(i64, i64), { COS_M - 1 }>` if the repo already wants that dependency, but fixed array is preferred to avoid dependency churn.

### 5. `solve_fair_coupon_deim()` owner

Location: [autocall_v2.rs](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2482)

Refactor intent:

- Accept read-only leg views plus caller-provided `p_red_v` / `p_red_u`.
- Convert all returned `Vec` scratch to caller-owned arrays / out-buffers.
- Keep terminal and backward buffers fixed-size.

Sites:

- `val_u_full` at [autocall_v2.rs:2502](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2502)
  Current: `Vec<i64>` length `s = 50`
  Proposed: `[i64; generated::N_STATES]`
  Classification: `stack-buffer`
  Downstream updates: `phi_transpose_times_v`

- `val_t_full` at [autocall_v2.rs:2503](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2503)
  Current: `Vec<i64>` length `50`
  Proposed: `[i64; generated::N_STATES]`
  Classification: `stack-buffer`
  Downstream updates: `phi_transpose_times_v`

- `v_u` and `v_t` at [autocall_v2.rs:2513](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2513) and [2514](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2514)
  Current: allocated by `phi_transpose_times_v() -> Vec<i64>`
  Proposed: `[i64; generated::D]` filled by `phi_transpose_times_v_out(...)`
  Classification: `stack-buffer`
  Downstream updates: `phi_transpose_times_v`

- `hybrid_at` at [autocall_v2.rs:2534](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2534)
  Current: `Vec<i64>` length `15`
  Proposed: `[i64; generated::D]`
  Classification: `stack-buffer`
  Downstream updates: none outside `solve_fair_coupon_deim`

- `new_u_at` at [autocall_v2.rs:2554](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2554)
  Current: `Vec<i64>` length `15`
  Proposed: `[i64; generated::D]`
  Classification: `stack-buffer`
  Downstream updates: `deim_matvec6_out`

- `new_t_at` at [autocall_v2.rs:2555](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2555)
  Current: `Vec<i64>` length `15`
  Proposed: `[i64; generated::D]`
  Classification: `stack-buffer`
  Downstream updates: `deim_matvec6_out`

- `e_t`, `v_u_at`, `v_t_at`, `hybrid_red`, `e_u`, `e_u_at`, `e_t_at` at [autocall_v2.rs:2528](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2528), [2532](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2532), [2533](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2533), [2542](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2542), [2543](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2543), [2550](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2550), [2551](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2551)
  Current: allocated by repeated `deim_matvec6() -> Vec<i64>`
  Proposed: predeclared `[i64; generated::D]` buffers filled by `deim_matvec6_out(...)`
  Classification: `stack-buffer`
  Downstream updates: `deim_matvec6`

### 6. Helper kernels

#### `phi_transpose_times_v()`

Location: [autocall_v2.rs:2617](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2617)

- Current signature: `fn phi_transpose_times_v(...) -> Result<Vec<i64>, SolMathError>`
- Proposed signature: `fn phi_transpose_times_v_out(phi: &[i64], v: &[i64], out: &mut [i64; generated::D]) -> Result<(), SolMathError>`
- Classification: `stack-buffer`
- Affected call sites: [2513](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2513), [2514](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2514)

#### `deim_matvec6()`

Location: [autocall_v2.rs:2635](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2635)

- Current signature: `fn deim_matvec6(...) -> Result<Vec<i64>, SolMathError>`
- Proposed signature: `fn deim_matvec6_out(mat: &[i64], v: &[i64; generated::D], out: &mut [i64; generated::D]) -> Result<(), SolMathError>`
- Classification: `stack-buffer`
- Affected call sites: all E11 DEIM matvec call sites in [2528](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2528) through [2572](/Users/dominic/colosseumfinal/crates/halcyon_sol_autocall_quote/src/autocall_v2.rs:2572)

## Refactor Order

### Group 1. Zero-allocation const views

Files:

- `crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs`
- `crates/halcyon_sol_autocall_quote/src/autocall_v2.rs`

Work:

- Add `*Const<'a>` runtime view types.
- Rewrite `generated_quote_context()` to `generated_quote_context_const()`.
- Update `solve_fair_coupon_e11_from_const()` and `solve_from_context()` to pass const views.

Why first:

- This removes the current hard OOM site before any solver scratch work.
- It minimizes churn by keeping host-side training structs unchanged.

Checkpoint:

- `preview_quote` should get past `cu_trace:e11:before_generated_context`.

### Group 2. E11 live-operator scratch buffers

Files:

- `crates/halcyon_sol_autocall_quote/src/autocall_v2.rs`

Work:

- Replace `dp_vals`, `c_coeffs_hp`, `p_red_*`, and `nig_cdf_cos_at` coeff scratch.
- Decide `p_hp` implementation based on frame size:
  - preferred first attempt: fixed `[i128; 225]` helper-local array
  - fallback if frame too large: smaller row-scratch accumulation

Why second:

- This isolates the live-operator assembly path before the DEIM backward pass changes.

Checkpoint:

- `cargo check -p halcyon_sol_autocall_quote`
- `preview_quote` reaches `cu_trace:e11:after_p_red_assembly`

### Group 3. DEIM backward pass buffer rewrite

Files:

- `crates/halcyon_sol_autocall_quote/src/autocall_v2.rs`

Work:

- Replace helper-returned `Vec`s with out-buffer helpers.
- Replace terminal/full-state/reduced scratch with arrays.
- Remove `clone_deim_leg` and `patched` construction.
- Make solver accept read-only leg refs plus `p_red_v` / `p_red_u`.

Why third:

- This is the most compile-error-heavy step, so it should happen after inputs are already stable.

Checkpoint:

- `cargo check -p halcyon_sol_autocall_quote`
- `cargo test -p halcyon_sol_autocall_quote --test smoke`

### Group 4. Deploy and measure

Work:

- run off-chain parity validation first on sigma sweep `0.80, 1.00, 1.17, 1.50, 2.00`
- Rebuild deployable program
- Redeploy devnet
- Re-run preview with current CU markers
- Verify no residual heap sites remain in the affected runtime path

Commands:

- `rg -n "to_vec\\(|collect::<Vec|Vec::new\\(|vec!\\[" crates/halcyon_sol_autocall_quote/src/autocall_v2.rs crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs`
- `scripts/anchor_build_checked.sh halcyon_sol_autocall`
- devnet preview with existing marker logs

Success criteria:

- off-chain sigma sweep matches pre-refactor reference to within SCALE_6 quantization
- `preview_quote` completes on devnet
- E11 path returns a fair coupon
- full CU trace is available
- no runtime-path `Vec` allocation remains in the fixed-table E11 path

## Review Notes

- The safest bounded implementation is to add const-view runtime types instead of changing all existing heap-owning structs globally.
- `p_hp` is the only buffer that needs frame-size attention. Everything else is comfortably small.
- `clone_deim_leg` should be deleted from the runtime path rather than translated one-for-one.
- The off-chain training code in `autocall_v2_e11.rs` remains host-only and can continue using heap-owning vectors.
