# Fix prompt: SOL Autocall POD-DEIM build.rs precompute

A self-contained brief for the session that implements this fix. Can be handed
to an agent (infrastructure work only) or executed by Dom. Math decisions are
flagged as such; infrastructure is everything else.

---

## What's broken

SOL Autocall's on-chain `preview_quote` blows past Solana's 1.4M CU ceiling.
Devnet test result on 2026-04-21 after all other bootstrap gaps resolved:

```
Program 6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8 invoke [1]
Program log: Instruction: PreviewQuote
Program 6DfpE... consumed 1399850 of 1399850 compute units
Program 6DfpE... failed: exceeded CUs meter at BPF instruction
```

The whitepaper says the primary pricer should cost ~350K CU. There is a
design-vs-implementation gap.

## Read these first (in order)

1. `halcyon_whitepaper_v9.md:140` — single paragraph. States 350K CU target
   for POD-DEIM primary, 809K CU for Richardson fallback.
2. `halcyon_whitepaper_v9.md:210` — on/off-chain split table. Row:
   *"POD-DEIM online solve (SOL Autocall) | POD-DEIM training
   (SVD, DEIM cell selection)"*.
3. `product_economics/sol_autocall_math_stack.md` §4 (lines 86–214) —
   complete description of the POD-DEIM decomposition: what Phi is, what
   the DEIM points are, what the projected modes are, what the online solve
   is.
4. `product_economics/sol_autocall_math_stack.md:210` — *"Offline cost:
   K full-order backward passes + 2 SVDs + DEIM selection. Runs once per
   alpha/beta calibration."*
5. `product_economics/sol_autocall_math_stack.md:375-390` — on/off-chain
   split for SOL autocall specifically.
6. `ARCHITECTURE.md:100-109` — "Flagship quote precision contract."
   Describes the pattern for shipping precomputed data as const on-chain.
   Model your approach on this.
7. `crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs` — the existing
   code.

## What the existing code does (the gap)

`crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs`:

- **Line 38:** `static E11_QUOTE_CACHE: OnceLock<Mutex<HashMap<...>>>` —
  process-local memoisation of `E11QuoteContext`. Works off-chain because
  host processes persist state across calls. Never survives in BPF (each
  instruction is a fresh process).
- **Lines 127–290:** `build_quote_context()` — runs the full offline training
  pipeline: 25 training sigmas × 2 backward passes = 50 full-order N=50
  backward passes, then SVD #1 on the 2500×25 snapshot matrix, then SVDs #2
  and #3 for the POD basis, then DEIM cell selection. This is what the math
  stack §4.7 calls "Offline cost." Currently runs at quote time.
- **Line 108:** `solve_from_context()` — the online solve: reads the
  reduced-order objects built by `build_quote_context` and does the actual
  12 NIG CDF + 12 mode additions + 8 backward steps + payoff at d DEIM
  points. This part is correct and should stay on-chain.
- **Lines 47–106:** `solve_fair_coupon_e11_cached()` — wraps build + solve
  with the HashMap memoisation.

**Consumer:** `programs/halcyon_sol_autocall/src/pricing.rs:139` calls
`solve_fair_coupon_e11_cached(...)` inside `solve_quote()`. There's a
Richardson fallback at line 146 (`.unwrap_or_else(|_| gated.result.clone())`)
that activates on `Err`.

## Target design

POD-DEIM training runs **once at Halcyon build time**, not at quote time.
The outputs are serialised as `const` arrays in a generated Rust source file
and `include!`'d into the quote crate. The on-chain solve reads the consts
and does only the 350K-CU online work.

**What gets precomputed and shipped as const:**

Using the math stack's notation with N=50 states, d=15 POD dims, M=12 DEIM
cells, training sigma grid of K=25 values spanning [0.50, 2.50]:

1. `POD_BASIS_PHI: [[i64; 15]; 50]` — the 50×15 POD basis Phi. First d cols
   of the SVD of the snapshot matrix S (math stack §4.2, line 104–110).
2. `PROJECTED_MODES: [[[i64; 15]; 15]; 12]` — the 12 reduced d×d mode
   matrices `Phi^T · dP_k · Phi` for k ∈ [0, M). These are the
   sigma-independent projected operator modes from math stack §4.4 line 162.
3. `DEIM_OP_POINTS: [(u16, u16); 12]` — the 12 operator DEIM cell indices
   (row, col) selected greedily per math stack §4.3 lines 144–152.
4. `DEIM_V_POINTS: [u16; 15]` — the d=15 payoff DEIM row indices selected
   by the same greedy algorithm applied to the POD basis Phi (math stack
   §4.6, line 189–197).
5. `PHI_AT_DEIM_INV: [[i64; 15]; 15]` — the inverse of the d×d submatrix of
   Phi at the DEIM rows (invertible by construction per §4.6 line 197).
6. `P_REF: [[i64; 50]; 50]` — the reference transition matrix used as the
   mean in the snapshot deviation (already exists in
   `build_quote_context:184`, `p_ref_flat`).
7. `GRID_INFO` — the `MarkovGridInfo` built from the max-sigma NIG params.
   This is also currently built online (line 142) and depends on α, β,
   ref_step_days, N, and the contract barriers. It's a pure function of
   compile-time constants, so it can move to const too. Serialise the
   fields of `MarkovGridInfo` as consts.

**Scale decision (math decision — Dom-only):**

Need to pick a fixed-point scale for the matrix entries. Options:

- **S6** (1.0 = 1_000_000, `i64`) — same scale as prices, easier to read,
  but 6 decimal digits of precision may be tight for 15×15 matrix-vector
  products with 50×15 basis projections (accumulated error could exceed
  1 bps on the final coupon).
- **S12** (1.0 = 1e12, `i64`) — matches the EWMA variance code
  (`programs/halcyon_kernel/src/instructions/oracle/update_ewma.rs:80`,
  `LAMBDA_S12: i128 = 940_000_000_000`). More headroom.
- **Mixed: S12 for matrix entries + projected modes, S6 for DEIM cell
  indices (integers anyway).**

The backward recursion operates in `i128` accumulators in either case. The
regression guard is
`crates/halcyon_sol_autocall_quote/tests/smoke.rs` + the existing
`fixed_point_drift_coupon_matches_legacy_f64_reference_sweep` sweep — both
must pass after the conversion.

**Hash commitment (parity with flagship K12 table):**

Emit a `const POD_DEIM_TABLE_SHA256: [u8; 32]` that covers all the above
consts. Store the expected commitment in `ProtocolConfig` (mirror
`k12_correction_sha256` / `daily_ki_correction_sha256` fields). Verify at
program startup / first call per math stack §10 audit requirements.

## Implementation plan

### Step 1: Extract the offline pipeline into a standalone codegen

Create `crates/halcyon_sol_autocall_quote_training/` as a new workspace
member — a host-only binary crate that depends on `halcyon_sol_autocall_quote`
(host build) and produces the generated const file.

Why a separate crate, not `build.rs` directly:
- `build.rs` with heavy nalgebra dependencies slows every clean build.
- Training is parameterised by α, β, barriers, n_obs, ref_step_days. These
  are stable across the Colosseum demo window. Re-run manually when
  calibration changes.
- Output is a checked-in generated file — reviewers can inspect it, CI
  can diff-check it, auditors can reproduce it.

Binary entry point:

```rust
// crates/halcyon_sol_autocall_quote_training/src/main.rs
fn main() -> anyhow::Result<()> {
    // MATH DECISION: inputs come from product_economics docs
    let params = TrainingParams {
        alpha_s6: 13_040_000,                       // whitepaper §3.3 (NIG α=13.04)
        beta_s6: 1_520_000,                         // whitepaper §3.3 (NIG β=1.52)
        reference_step_days: 2,                     // 16-day / 8 observations
        n_states: 50,
        d: 15,
        m: 12,
        n_obs: 8,
        no_autocall_first_n_obs: 2,                 // 2-day lockout
        knock_in_log_6: KNOCK_IN_LOG_6,             // reuse from autocall_v2_e11.rs
        autocall_log_6: AUTOCALL_LOG_6,
        training_sigmas: sigma_training_grid(),     // existing helper
    };
    let artefacts = run_training(&params)?;
    write_generated_file(
        Path::new("crates/halcyon_sol_autocall_quote/src/generated/pod_deim_table.rs"),
        &artefacts,
    )?;
    let sha = sha256(&artefacts);
    write_generated_file(
        Path::new("crates/halcyon_sol_autocall_quote/src/generated/pod_deim_table_sha256.rs"),
        &sha,
    )?;
    Ok(())
}
```

`run_training` is 90% the body of the existing `build_quote_context` —
extract it into a host-only function returning `(Phi, projected_modes,
deim_op_points, deim_v_points, phi_at_deim_inv, p_ref, grid_info)` as f64
values.

`write_generated_file` serialises each output to Rust source with fixed-
point conversion. Emit values with full precision (no trailing-zero
elision, no scientific notation) so diffs are reviewable.

### Step 2: cfg-gate the old training path

In `autocall_v2_e11.rs`:

- Wrap `build_quote_context`, `solve_fair_coupon_e11_cached`,
  `E11_QUOTE_CACHE`, and all nalgebra usage in
  `#[cfg(not(target_os = "solana"))]`.
- Keep `solve_from_context` and its math (lines 108-125 + inner helpers like
  `solve_e11_reduced_order_f64` or however the inner solve is named in the
  current code) available in both builds — that's the online path.
- Add a new on-chain entrypoint:
  ```rust
  #[cfg(target_os = "solana")]
  pub fn solve_fair_coupon_e11_from_const(
      sigma_ann_6: i64,
      alpha_6: i64,
      beta_6: i64,
      reference_step_days: i64,
      contract: &AutocallParams,
  ) -> Result<AutocallPriceResult, AutocallV2Error> {
      // Build an E11QuoteContext view over the const tables.
      let context = E11QuoteContextConst {
          phi: &generated::POD_BASIS_PHI,
          projected_modes: &generated::PROJECTED_MODES,
          deim_op_points: &generated::DEIM_OP_POINTS,
          deim_v_points: &generated::DEIM_V_POINTS,
          phi_at_deim_inv: &generated::PHI_AT_DEIM_INV,
          p_ref: &generated::P_REF,
          grid_info: &generated::GRID_INFO,
      };
      solve_from_const_context(&context, sigma_ann_6, alpha_6, beta_6,
                               reference_step_days, contract)
  }
  ```
- Host builds still expose `solve_fair_coupon_e11_cached` for tests,
  CLI preview, and WASM pricing. Reference implementation preserved.

### Step 3: Wire into the consumer

`programs/halcyon_sol_autocall/src/pricing.rs:139` currently calls
`solve_fair_coupon_e11_cached`. Change to use a helper that dispatches:

```rust
#[cfg(target_os = "solana")]
use halcyon_sol_autocall_quote::autocall_v2_e11::solve_fair_coupon_e11_from_const as solve_e11;

#[cfg(not(target_os = "solana"))]
use halcyon_sol_autocall_quote::autocall_v2_e11::solve_fair_coupon_e11_cached as solve_e11;
```

The call site and its error fallback stay unchanged.

### Step 4: Hash commitment wiring

Mirror the flagship K12 pattern:

- `programs/halcyon_kernel/src/state/protocol_config.rs` already has
  `k12_correction_sha256: [u8; 32]` and `daily_ki_correction_sha256:
  [u8; 32]`. Add `pod_deim_table_sha256: [u8; 32]`.
- Accept it as an argument to `initialize_protocol` /
  `set_protocol_config` (follow existing field additions).
- On the product side, add a one-time startup check: on first
  `preview_quote` call after deploy, verify
  `sha256(generated::POD_DEIM_TABLE_BYTES) == protocol_config.pod_deim_table_sha256`.
- Migration: existing devnet `ProtocolConfig` has the field zeroed.
  `set_protocol_config` admin call updates it after deploy.

### Step 5: Training artefact reproducibility

Auditor replay requirement per math stack §10 audit notes:

- Check the generated file into git (`crates/halcyon_sol_autocall_quote/src/generated/pod_deim_table.rs`).
- CI job: re-run the training binary, compare to the checked-in file, fail
  if different. Ensures no undetected drift.
- Training inputs (α, β, barriers, etc.) must be deterministic. If
  `sigma_training_grid()` uses any randomness (shouldn't), make it
  deterministic.

### Step 6: Deprecate the `.bss` section-strip workflow

Once E11 is gated out of BPF builds, no `.bss` sections with long names
should appear in `target/deploy/halcyon_sol_autocall.so`. Verify:

```bash
llvm-objdump --section-headers target/deploy/halcyon_sol_autocall.so \
  | grep -E "bss|long name"
```

Should return empty. Remove the section-strip steps from any deploy runbook
/ Makefile that still references them. Dom's ops notes reference the strip
as a workaround — update `docs/things-to-do-before-launch.md §3.0` to mark
it obsolete.

### Step 7: Redeploy + verify

1. `anchor build` clean (no llvm-objcopy step)
2. `solana program deploy --url devnet --program-id target/deploy/halcyon_sol_autocall-keypair.json target/deploy/halcyon_sol_autocall.so`
3. Admin tx: `halcyon set-protocol-config --pod-deim-table-sha256 <hash>`
4. Ensure `update-ewma` for SOL has run recently (vault_sigma fresh)
5. Ensure `fire-regime --product sol` has run (regime_signal fresh)
6. Ensure price_relay is running (Pyth prices <30s stale)
7. Run preview:
   ```bash
   halcyon --rpc <devnet> --keypair <admin> \
     preview --pyth-sol ES2Q48KYND5GKYynzJT8RUm3kzwru1stmHnPNyrdhASn 5000000
   ```
8. Verify CU usage in logs is in the whitepaper's 350K ballpark (with
   some overhead for Anchor deserialisation + return data encoding, ≤500K
   is the acceptable envelope).

### Step 8: Regression guards

- `crates/halcyon_sol_autocall_quote/tests/smoke.rs` — should still pass
  with both host and BPF solve paths. Add a new test that runs both paths
  at a grid of sigmas and asserts the coupon difference is below a chosen
  tolerance (MATH DECISION: what tolerance — probably ≤1 bps on
  `fair_coupon_bps`).
- `fixed_point_drift_coupon_matches_legacy_f64_reference_sweep` if it
  applies to SOL autocall (grep for it) — rerun to confirm no drift
  introduced by the S6/S12 conversion.

## What NOT to do

- Do not cfg-gate E11 out on-chain with a Richardson-only fallback as the
  permanent fix. That contradicts the whitepaper's "POD-DEIM online solve
  | On-chain" commitment in the on/off-chain split table.
- Do not try to make the nalgebra SVD path `const fn`. Not possible; not
  the point.
- Do not delete the off-chain `solve_fair_coupon_e11_cached` — it's the
  reference implementation and it's what CLI preview, smoke tests, WASM
  pricing, and the 1,638-note backtest in the economics report use.
- Do not change the protocol parameters (α, β, barriers, n_obs,
  ref_step_days) as part of this fix. The gap is purely about *where* the
  training runs, not about what it trains on. Any parameter change is a
  separate quant decision (Dom-only).
- Do not remove the HashMap-memoised path unconditionally — host builds
  legitimately benefit from it (CLI preview iterates on sigma; batch
  backtests hit the same α/β thousands of times).

## Scope / effort

1–3 focused days. Breakdown:

- Training crate extraction + generated file emission: 0.5 day
- Fixed-point conversion + scale choice + tests: 0.5–1 day
- On-chain solve refactor (reading consts instead of building context): 0.5 day
- Hash commitment wiring through `ProtocolConfig`: 0.5 day
- CI + regression test + deploy + verify: 0.5 day
- Slack for debugging SVD sign conventions (POD basis has sign ambiguity —
  the common gotcha if the const values don't match the live path): 0.5 day

## Success criteria

1. `preview` on SOL autocall completes on devnet within the whitepaper's
   CU envelope (≤500K CU including overhead; target 350K matches the
   whitepaper claim).
2. `llvm-objdump --section-headers halcyon_sol_autocall.so` shows no
   `.bss.*` sections. `anchor deploy` works with no section-strip step.
3. Off-chain paths (CLI preview, smoke tests, WASM) still pass the
   existing regression sweep without tolerance relaxation.
4. New host-vs-BPF parity test passes at chosen tolerance across the
   [0.50, 2.50] sigma band.
5. `ProtocolConfig.pod_deim_table_sha256` matches the generated table hash.
6. An auditor running the training binary from clean checkout gets a
   byte-identical generated file.

## Pointers for the first 10 minutes of the session

```bash
# Read the whitepaper paragraph
sed -n '140p' halcyon_whitepaper_v9.md

# Read the math stack section 4 in full
sed -n '86,214p' product_economics/sol_autocall_math_stack.md

# Read the current training code end-to-end
cat crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs

# Look at flagship's precomputed table pattern for reference
grep -rn "k12_correction_sha256\|K12_CORRECTION\|include!" programs/halcyon_flagship_autocall/ crates/halcyon_flagship_quote/ | head -20

# Check what's in ProtocolConfig today
grep -A3 "pub struct ProtocolConfig" programs/halcyon_kernel/src/state/protocol_config.rs | head -50
```

The math already exists and is correct. This is a code-organisation fix —
move the offline training from runtime to build time, wire the consts
through.
