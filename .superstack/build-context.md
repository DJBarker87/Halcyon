# Halcyon — Build Context

## review

- **security_score**: B+
- **quality_score**: A-
- **ready_for_mainnet**: false (after P1 fixes → true)
- **reviewed_at**: 2026-04-21
- **reviewed_commit**: 6ef3e8a

### per_program_security

- `halcyon_kernel`: A
- `halcyon_flagship_autocall`: B+
- `halcyon_il_protection`: B+
- `halcyon_sol_autocall`: B

### findings

| # | Severity | Category   | File                                                                                      | Description                                                                                  | Fix                                                                         |
|---|----------|------------|-------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------|
| 1 | High     | Security   | `halcyon_flagship_autocall/observation.rs`, `record_coupon_observation.rs:151`, `settle.rs:143` | Pyth staleness relies on observation window (±5/15 min), not absolute `now - publish_ts`. | Add `require!(now - publish_ts ≤ pyth_quote_staleness_cap_secs)` post-read. |
| 2 | High     | Correctness | `halcyon_sol_autocall/pricing.rs:177`                                                    | `solve_e11(...).unwrap_or_else(...)` silently falls back to Richardson result.             | Emit `E11FallbackUsed` event or return error; flag it on `QuoteRecorded`.    |
| 3 | High     | Security   | `halcyon_kernel/write_regression.rs:67-88`                                                | v0 regression init accepts unbounded `window_{start,end}_ts`.                                | Validate `> 0`, `end > start`, `end ≤ now + GRACE` on every write incl. v0. |
| 4 | Medium   | Security   | `halcyon_il_protection/settle.rs:188` (`price_s6_to_s12`)                                 | No defensive `price_s6 > 0` check before s12 conversion.                                     | Add `require!(price_s6 > 0, InvalidEntryPrice)` at fn entry.                 |
| 5 | Medium   | Compute    | `halcyon_sol_autocall/pricing.rs:40-51,233,239`                                          | `cu_trace()` / `sol_log_compute_units_()` live in mainnet hot path.                          | Gate behind `#[cfg(feature = "cu-trace")]`; compile mainnet without.        |
| 6 | Medium   | Correctness | `halcyon_sol_autocall/record_observation.rs:176`                                         | Replay idempotency returns Ok silently — no event, no monitoring signal.                     | Emit `ObservationReplayed { expected, submitted }` before early-return.      |
| 7 | Medium   | Correctness | `halcyon_flagship_autocall/pricing.rs:206` + state setup                                 | `offered_coupon_bps_s6` has no init-time upper bound.                                        | On `accept_quote`: `require!(offered_coupon_bps_s6 ≤ 10_000_000)`.          |
| 8 | Low      | Correctness | `halcyon_flagship_autocall/reconcile_coupons.rs`                                          | `next_coupon_index` not bounded at handler entry.                                            | First-line: `require!(pt.next_coupon_index ≤ MONTHLY_COUPON_COUNT)`.         |
| 9 | Low      | Correctness | `halcyon_sol_autocall/accept_quote.rs`                                                    | `observation_schedule` not checked for monotonic + future timestamps.                        | Validate `schedule[i] < schedule[i+1]` and `schedule[0] > now`.              |

### architectural_risks

- **POD-DEIM provenance**: whitepaper says offline training; mainnet uses `solve_fair_coupon_e11_from_const` (correct). Document SHA of precompiled table in `ProtocolConfig` and submission README; judge will ask.
- **No global keeper pause**: `rotate_keeper` works post-compromise, no kill-switch before. Consider `keepers_paused` flag on `ProtocolConfig`.
- **Self-hosting marketing surface on home NUC**: operational SPOF during Colosseum judging. Keep keepers on NUC; push landing page to Vercel/Cloudflare.

### clean_areas

- Checked arithmetic everywhere on value-sensitive paths.
- Signer/authority checks stricter than `Signer<'info>` (uses `require_keys_eq!` against registry).
- PDA bump canonicalization + seeds with unique identifiers.
- `.env` not in git; `.gitignore` covers keeper configs; CI pins action SHAs; Anchor.toml uses user keypair.
- Frontend config locked to compile-time allowlist; genesis-hash cluster check.
- Keeper daemons: exponential backoff + hardened systemd units (`ProtectSystem=strict`, `NoNewPrivileges`, `MemoryDenyWriteExecute`).
- CI invariants: pure-Rust quote crates cannot depend on `anchor-lang`/`solana-program`; precision-baseline regression guard; `cargo audit` with documented waivers.

### next_phase

After P1 fixes (#1, #2, #3) + #5, proceed to Phase 3:
- `/deploy-to-mainnet` — production deployment checklist
- `/submit-to-hackathon` — Colosseum submission builder
- `/create-pitch-deck` — structured pitch deck

HTML artifact at `.superstack/review.html`.
