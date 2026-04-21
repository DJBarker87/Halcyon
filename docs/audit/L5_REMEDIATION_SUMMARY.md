# Layer-5 Audit Remediation — Summary

Index of what changed, per finding, with pointers into code and tests.
The f64-in-flagship-pricing finding is handled by a separate prompt and
is out of scope here.

## Status by finding

| # | Finding | Status | Notes |
|---|---------|--------|-------|
| F1 | Flagship hedge executor | **Scaffolded** | Crate skeleton, staleness gates, 2D composition, `--dry-run` mode, `l4-hedge-keeper-check` target. Live Jupiter submission deferred to follow-up; flagship stays paused-public until a devnet rebalance cycle completes end-to-end. |
| F2 | Pyth `publish_time` staleness | **Closed** | `AggregateDelta.pyth_publish_times` added. `write_aggregate_delta` enforces per-feed staleness cap + clock-skew + strict monotonicity between writes. |
| F3 | Frontend runtime config hardening | **Closed** | `ALLOWED_CONFIGS` allowlist. Only cluster id read from localStorage. Genesis-hash verification on load. Explicit re-consent modal for cluster switches. Playwright tests for poisoned-storage + modal + genesis-mismatch paths. |
| F4b | AggregateDelta keeper signature | **Closed** | `AggregateDelta.keeper_signature` + `.sequence` added. Ed25519 precompile introspected on-chain via instructions sysvar; signed bytes are the fixed-width canonical encoding in `halcyon_common::aggregate_delta_signing`. External Python verifier included. |
| F4a | Merkle publication + IPFS | **Closed** | `AggregateDelta.publication_cid` added. Delta keeper pins canonical JSON to Pinata (`PINATA_JWT` env var), commits CID on-chain. Auditor doc explains the round-trip. |
| F5 | `write_aggregate_delta` product binding | **Closed** | Already bound to `ProductRegistryEntry` via PDA seeds and `active` check; this pass adds `!paused` to match audit test cases. |
| F6 | `reap_quoted` canonical PDA seeds | **Closed (pre-existing)** | `vault_state` and `product_registry_entry` already carry `seeds = […], bump` constraints. |
| F7 | `update_lookup_table` owner validation | **Closed (pre-existing)** | Already validates `lookup_table_account.owner == ADDRESS_LOOKUP_TABLE_PROGRAM_ID` in parity with `register_lookup_table`. |

## Code paths changed

### AggregateDelta extensions (F2 + F4b + F4a)
- `crates/halcyon_common/src/aggregate_delta_signing.rs` — canonical 147-byte signed-message encoder, byte layout documented.
- `programs/halcyon_kernel/src/state/aggregate_delta.rs` — struct v1 → v2 with four new fields.
- `programs/halcyon_kernel/src/instructions/oracle/write_aggregate_delta.rs` — new args, protocol_config + instructions_sysvar accounts, Ed25519 introspection, monotonicity, staleness.
- `programs/halcyon_kernel/src/errors.rs` — 7 new KernelError variants.
- `crates/halcyon_client_sdk/src/aggregate_delta.rs` — `build_signed_write_aggregate_delta_ixs` helper, `encode_publication_cid` helper.
- `keepers/delta_keeper/src/main.rs` — canonical signing, Pinata pin, paired-ix submission.
- `programs/halcyon_kernel/LAYOUTS.md` — updated AggregateDelta table (passes `make layouts-check`).

### Frontend hardening (F3)
- `frontend/lib/allowed-configs.ts` — compile-time allowlist + genesis hashes + default cluster.
- `frontend/lib/runtime-config.tsx` — reads only cluster id, fetches `getGenesisHash`, exposes pending-change workflow.
- `frontend/lib/runtime-config-schema.ts` — now trivial (isClusterId + storage key only).
- `frontend/components/settings-panel.tsx` — cluster picker + read-only pinned-wiring display.
- `frontend/components/cluster-switch-modal.tsx` — explicit re-consent modal.
- `frontend/components/app-shell.tsx` — genesis-check error banner.
- `frontend/tests/runtime-config.spec.ts` — rewritten against post-audit behaviour.

### Flagship hedge keeper (F1 scaffold)
- `keepers/flagship_hedge_keeper/Cargo.toml` — new crate.
- `keepers/flagship_hedge_keeper/src/main.rs` — scaffold with staleness gates, 2D composition, rebalance decision logic, `--dry-run` mode.
- `Cargo.toml` — workspace membership.
- `Makefile` — `l4-hedge-keeper-check` target, included in `l4-gate`.
- `config/examples/flagship_hedge_keeper.example.json` — config template.
- `docs/audit/OPEN_QUESTIONS.md` — records the unpause predicate.

## Test coverage added

Rust unit tests (24 new):
- `halcyon_common::aggregate_delta_signing` — 3 tests (encoding determinism, layout, sequence differentiation).
- `halcyon_client_sdk::aggregate_delta` — 3 tests (CID round-trip, oversize rejection, empty rejection).
- `halcyon_kernel::instructions::oracle::write_aggregate_delta` — 6 tests (Ed25519 precompile parser: happy path, pubkey mismatch, message mismatch, cross-instruction reference, wrong num_signatures, truncated data).
- `flagship_hedge_keeper` — 7 tests (2D composition with positive/negative beta and truncation, rebalance triggers on cooldown + breach, skip inside bands, spot drift calculation).
- Plus 5 pre-existing delta_keeper tests still passing.

Playwright tests (4 new in `runtime-config.spec.ts`):
- Unknown cluster id in localStorage falls back to default.
- Arbitrary fields in localStorage are ignored.
- Cluster change requires explicit modal confirmation.
- Genesis-hash mismatch surfaces error banner.

## Known follow-up items

1. **F1 live Jupiter submission.** The flagship hedge keeper currently
   logs targets in dry-run mode. Full `prepare_hedge_swap` → Jupiter →
   `record_hedge_trade` Flash-Fill v0 composition, ALT resolution, and a
   devnet integration test are the next session's work. Flagship stays
   paused-public until a complete devnet rebalance cycle lands.

2. **Localnet Anchor integration test for F4b/F4a/F2.** The Rust unit
   tests cover the byte-level Ed25519 parser, canonical encoding, and CID
   packing; an end-to-end `anchor test --skip-lint` run exercising the
   full write → verify → fetch-from-IPFS → Merkle-round-trip path is a
   follow-up. (Requires building an Ed25519 precompile instruction in TS
   or driving the keeper binary from the test runner.)

3. **TS tests for F5 paused-product and F7 invalid-ALT rejections.** The
   on-chain constraints are in place; adding TS assertions that a paused
   product write or a non-ALT-owned update is rejected with the right
   error code would round out coverage.

4. **Document sweep (integration_architecture.md / ARCHITECTURE.md /
   THREAT_MODEL.md).** The LAYOUTS.md update landed; the larger narrative
   docs benefit from a dedicated reading pass to remove hedging language
   (e.g. "partial auditability", "mostly deterministic") now that the
   code backs those claims fully.

5. **Pre-existing SBF stack-usage verifier errors.** `halcyon_flagship_quote::worst_of_c1_filter::quote_c1_filter` and `solmath_core::n3_cv_premium::compute_n3_premium` emit verifier stack-overwrite warnings on `anchor build`. These predate this work and are handled by the f64-in-flagship-pricing prompt / IL Protection follow-up; they do not block L5 audit closure.
