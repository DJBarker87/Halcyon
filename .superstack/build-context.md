# Halcyon — Build Context

## review

- **security_score**: A-
- **quality_score**: A-
- **ready_for_mainnet**: false (after remaining #3 upper-bound + #5 cu_trace gating → true)
- **reviewed_at**: 2026-04-22
- **reviewed_commit**: 41bdb2f (+ uncommitted working tree)
- **prior_review**: 2026-04-21 @ 6ef3e8a (B+ / A-)

### per_program_security

- `halcyon_kernel`: A-
- `halcyon_flagship_autocall`: A-
- `halcyon_il_protection`: B+ (unchanged — not re-reviewed; #4 still pending)
- `halcyon_sol_autocall`: A-

### carry_forward_status

| # | From 04-21    | Status          | Notes                                                                                                     |
|---|---------------|-----------------|-----------------------------------------------------------------------------------------------------------|
| 1 | Pyth absolute staleness           | **FIXED**       | `crates/halcyon_oracles/src/pyth.rs:92` enforces `(min..=max).contains(&publish_time)`; flagship observation.rs:33-39 and settle.rs:143-149 both use `observation_window_bounds()`. |
| 2 | E11 silent fallback               | **FIXED**       | `programs/halcyon_sol_autocall/src/pricing.rs:179-184` now calls `solve_keeper_deim(...).map_err(|_| error!(QuoteRecomputeMismatch))?` — fallback removed; architecture switched to keeper-fed POD-DEIM. |
| 3 | Regression init unbounded ts      | **PARTIAL**     | `write_regression.rs:62-65` enforces `end > start` unconditionally. Missing: `end ≤ now + GRACE` and `start > 0`. Keeper can still write `window_end_ts = i64::MAX` → DoS (subsequent writes fail monotonicity forever). |
| 4 | IL `price_s6 > 0` guard           | unverified      | Not re-checked this pass.                                                                                 |
| 5 | `cu_trace` in mainnet hot path    | **NOT FIXED**   | `pricing.rs:36-46` gates only on `#[cfg(target_os = "solana")]` — meaning BPF builds always log. Feature `cu_diagnostics` exists in Cargo.toml:19 but isn't used to gate calls. 5 call sites at lines 139/158/178/185 burn CU on mainnet. |
| 6 | Observation replay event          | unverified      | Not re-checked.                                                                                            |
| 7 | Offered-coupon init upper bound   | unverified      | Not re-checked.                                                                                            |
| 8 | `next_coupon_index` entry bound   | unverified      | Not re-checked.                                                                                            |
| 9 | Observation schedule monotonicity | unverified      | Not re-checked.                                                                                            |

### findings

| # | Severity | Category   | File                                                                                      | Description                                                                                  | Fix                                                                         |
|---|----------|------------|-------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------|
| A | Medium   | Security   | `halcyon_kernel/src/instructions/oracle/write_regression.rs:62-65`                        | `window_end_ts` has no upper bound; keeper can write `i64::MAX` and lock future monotonic updates (DoS of regression account). | Add: `require!(args.window_start_ts > 0 && args.window_end_ts ≤ now.saturating_add(REGRESSION_WINDOW_GRACE_SECS), HalcyonError::OracleTimestampOutOfRange);` — unconditional, before the v0/v1 branch. |
| B | Medium   | Compute    | `halcyon_sol_autocall/src/pricing.rs:36-46, 139, 158, 178, 185`                           | `cu_trace` only compiles out on non-BPF; on mainnet BPF builds all 5 log sites fire regardless of `cu_diagnostics` feature. | Change function body: replace `#[cfg(target_os = "solana")]` with `#[cfg(all(target_os = "solana", feature = "cu_diagnostics"))]`. Do NOT add feature to `default`. |
| C | Low      | Hardening  | `halcyon_kernel/src/instructions/admin/migrate_protocol_config.rs:180-190`                | Early-return path (already at `target_len`) deserializes ProtocolConfig but does not require `cfg.version == CURRENT_VERSION`. If a struct-size-preserving future bump ever coexists with stale data, silent no-op could mask it. | Before `return Ok(())`, add: `require!(cfg.version == ProtocolConfig::CURRENT_VERSION, KernelError::BadConfig);` |
| D | Low      | Hardening  | `halcyon_kernel/src/instructions/admin/set_protocol_config.rs` (premium-split validator)  | Sum-to-10k check doesn't bound individual components (belt-and-braces — sum=10k implies each ≤10k, so low-value). | `require!(senior_bps ≤ 10_000 && junior_bps ≤ 10_000 && treasury_bps ≤ 10_000, KernelError::BadConfig);` |

### architectural_risks (unchanged from 04-21)

- **POD-DEIM provenance**: keeper now loads reduced operators on-chain via new `write_reduced_operators.rs`. Whitepaper alignment improved. Document `pod_deim_table_sha256` from ProtocolConfig in submission README.
- **No global keeper pause**: `rotate_keeper` is post-compromise only. Consider `keepers_paused` flag on `ProtocolConfig`.
- **Self-hosting marketing surface on home NUC**: operational SPOF during Colosseum judging.

### clean_areas (newly verified this pass)

- **`write_reduced_operators.rs`** — regime-keeper authority check, abs-value bounds on `p_red` entries (`MAX_ABS_KEEPER_P_RED_ENTRY_Q20`), upload-state coherence correct.
- **`update_ewma.rs`** — variance arithmetic sound (squared ln_ratio ≥0), checked multiplications, monotonicity enforced, rate-limit staleness cap applied.
- **`migrate_protocol_config.rs`** (main path) — admin signer + stored-admin cross-check, rent top-up before realloc, in-place grow (never shrink), V5→V6 and V4→V6 both supported with complete field mapping.
- **flagship `accept_quote.rs` / `preview_quote.rs`** — `seeds::program` qualifiers added → PDA-confusion surface tightened.
- **SOL-autocall `accept_quote.rs`** — slippage bounds, pod_deim table hash, reduced-operator currency gate all verified before issuance.

### clean_areas (carried forward)

- Checked arithmetic throughout value paths; `require_keys_eq!` authority (stricter than `Signer`); canonical PDA bumps + unique seeds.
- `.env` gitignored; CI pins action SHAs; `cargo audit` with waivers; pure-Rust quote crates provably can't depend on `anchor-lang`/`solana-program`.
- Frontend locked to compile-time allowlist + genesis-hash cluster check.
- Keeper daemons hardened via systemd (`ProtectSystem=strict`, `NoNewPrivileges`, `MemoryDenyWriteExecute`).

### next_phase

Two fixes remain blocking mainnet-quality:
- **Finding A** (regression window upper bound) — 3-line handler change.
- **Finding B** (`cu_trace` feature gating) — 1-line `cfg` attribute change.

After A+B, proceed to:
- `/deploy-to-mainnet` — production deployment checklist
- `/submit-to-hackathon` — Colosseum submission builder
- `/create-pitch-deck` — structured pitch deck

HTML artifact at `.superstack/review.html`.
