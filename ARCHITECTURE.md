# Halcyon Architecture

This document is the Layer 5 auditor handoff for the current repo state. It is intentionally narrower than the whitepaper: it describes what is implemented, what runs off-chain, and which assumptions hold at launch.

## Scope

Halcyon v1 is a shared-kernel structured-products system on Solana with three product programs:

- `halcyon_sol_autocall`
- `halcyon_il_protection`
- `halcyon_flagship_autocall`

The shared state and capital logic live in `halcyon_kernel`. The keeper fleet and operator CLI are separate binaries. The production frontend copy lives in `frontend/`. The legacy browser-WASM demo remains in `app/` and is not part of the transacting surface.

## System Components

### On-chain programs

#### `programs/halcyon_kernel`

The kernel owns shared protocol state and capital accounting:

- `ProtocolConfig`
- `ProductRegistryEntry`
- `VaultState`
- `PolicyHeader`
- `FeeLedger`
- `KeeperRegistry`
- `LookupTableRegistry`
- `VaultSigma`
- `Regression`
- `RegimeSignal`
- product capital helpers such as `CouponVault` and `HedgeSleeve`

The kernel enforces:

- protocol-wide pause flags
- per-product active/paused status
- utilization and reservation caps
- premium split routing
- settlement and reservation accounting
- keeper role authorization
- ALT registry lookup for v0 transactions

The kernel does not price products. Product programs recompute quotes and call the kernel for reservation/finalization/settlement.

#### `programs/halcyon_sol_autocall`

This program handles:

- `preview_quote`
- `accept_quote`
- `record_observation`
- `settle`

It is principal-backed. Issuance escrows principal plus the vault premium share. Mid-life observation and hedge operations are keeper-driven.

#### `programs/halcyon_il_protection`

This program handles:

- `preview_quote`
- `accept_quote`
- `settle`

It is synthetic-only. The buyer pays premium and the shared vault carries the reserved liability. There is no observation keeper; settlement is European at expiry.

#### `programs/halcyon_flagship_autocall`

This program handles:

- `preview_quote`
- `accept_quote`
- coupon observation recording
- autocall observation recording
- KI event recording
- `settle`

It consumes the regression and aggregate-delta kernel state written by keepers. It uses the most complex lifecycle and is expected to remain paused on mainnet until legal and route-depth conditions are satisfied.

### Pure-Rust quote crates

The quote/math surface is split by product:

- `crates/halcyon_sol_autocall_quote`
- `crates/halcyon_il_quote`
- `crates/halcyon_flagship_quote`

Shared helpers live in:

- `crates/halcyon_common`
- `crates/halcyon_kernel_types`
- `crates/halcyon_oracles`
- `crates/halcyon_client_sdk`

The design intent is that program BPFs only link the product-specific quote logic they need.

#### Flagship quote precision contract

The shipped flagship issuance path is `corrected_coupon_bps_s6 -> quote_c1_filter` and now stays in `i64`/`i128` fixed-point throughout the on-chain build. The remaining `f64` helpers in `halcyon_flagship_quote` are gated to tests or non-Solana host builds.

`quote_c1_filter` returns fixed-point `C1FastQuote` outputs at `SCALE_6`. The regression guard for the fixed-point drift rewrite is `fixed_point_drift_coupon_matches_legacy_f64_reference_sweep` in the quote crate. That sweep covers issuance-time sigma from 8% to 80% annualised in 4% steps and compares the post-fix fixed-point path against the prior host-side `f64` drift reference.

Current bound from that sweep:

- max absolute fair-coupon error: `0.0` bps
- max relative error: `0.0`

That sweep does not vary entry prices because the shipped issuance quote path is normalised to entry spot ratios of `1.0`; entry prices affect note state and off-chain live-delta paths, not the on-chain issue-time `quote_c1_filter` input surface.

### Off-chain keepers

Five keepers exist in the repo:

- `keepers/observation_keeper`
- `keepers/hedge_keeper`
- `keepers/regime_keeper`
- `keepers/regression_keeper`
- `keepers/delta_keeper`

Current responsibilities:

- observation keeper: SOL autocall schedule execution
- hedge keeper: SOL spot hedge rebalancing through Jupiter
- regime keeper: IL regime/fvol writes
- regression keeper: flagship regression updates
- delta keeper: flagship aggregate delta and Merkle artifact generation

All keepers are external processes using registered keeper authorities from `KeeperRegistry`. They are not privileged beyond the specific roles the kernel checks.

### Operator and user clients

#### `tools/halcyon_cli`

The CLI is the operator surface for:

- protocol initialization
- product registration
- keeper rotation
- capital seeding and sleeve/coupon-vault funding
- quote preview
- SOL and IL issuance/settlement
- status inspection

#### `frontend/`

The Next.js app is the user-facing Layer 5 surface. It uses:

- Anchor-generated IDLs from `target/idl`
- `@solana/wallet-adapter-*`
- `simulateTransaction` for previews
- versioned transactions for issuance
- no indexer dependency

Pages:

- `/flagship`
- `/sol-autocall`
- `/il-protection`
- `/portfolio`
- `/vault`

The frontend stores runtime configuration per cluster in local storage so operators can switch between localnet, devnet, and mainnet without rebuilding.

## Control Plane

### Admin authority

`ProtocolConfig.admin` is the root authority. Layer 5 assumes this authority is transferred to a 3-of-5 or 4-of-7 multisig before mainnet issuance opens.

Admin actions include:

- protocol initialization
- product registration and pause/unpause changes
- keeper rotation
- protocol parameter updates
- lookup table registration

### Keeper authorities

Keeper authorities are distinct keys stored in `KeeperRegistry`. Recommended role mapping:

- `0` observation
- `1` regression
- `2` delta
- `3` hedge
- `4` regime

Launch guidance is fresh mainnet keys per role, not reused devnet keys.

### Product activation model

All products can be deployed and registered while only a subset is active for issuance. This is the expected mainnet flow:

- SOL Autocall: active
- IL Protection: active
- Flagship: deployed and registered, but paused until legal and liquidity conditions clear

## Transaction Model

### Quote preview

Users do not run client-side pricing. Preview flow is:

1. Frontend constructs a read-only instruction to `preview_quote`.
2. The wallet is not required.
3. The client uses `simulateTransaction`.
4. The Anchor return data is decoded using the product IDL.
5. The UI renders premium, liability, coupon or premium fraction, entry prices, and expiry.

This makes the product program the single source of truth for quotes.

### Issuance

Issuance flow is:

1. User previews quote.
2. User chooses slippage and guardrail bounds.
3. Client builds `accept_quote` as a v0 transaction.
4. The product-specific ALT registry is loaded from the kernel.
5. Wallet signs and submits.
6. Product program recomputes live quote and checks all preview bounds.
7. Product CPI-calls the kernel to reserve/finalize policy state.

The frontend never hand-rolls account deserializers; it consumes IDLs and Anchor coders.

### Settlement

Settlement is product-specific, but all terminal money movement is routed through `halcyon_kernel::apply_settlement`.

## Data Dependencies

### Oracle dependencies

Launch assumptions:

- SOL and USDC come from Pyth receiver accounts
- SPY, QQQ, and IWM also come from Pyth receiver accounts
- quote and settlement staleness caps are enforced by `ProtocolConfig`

The frontend does not cache oracle data. It depends on live on-chain preview calls.

### External services

Off-chain dependencies include:

- RPC endpoints for programs and keepers
- Jupiter for SOL hedge routing
- Raydium pool state as an economic dependency for IL
- external historical data sources for keepers where applicable

These are operational dependencies, not smart-contract authorities. Outages should fail closed, not silently continue on stale state.

## Deployment Topology

### Programs

Expected deployment set:

- `halcyon_kernel`
- `halcyon_sol_autocall`
- `halcyon_il_protection`
- `halcyon_flagship_autocall`

### Keepers

Recommended topology:

- one process per keeper binary
- at least two RPC endpoints available per environment
- independent alerting on heartbeat and failure budget exhaustion

### Frontend

The production frontend copy is `frontend/`. It should be hosted independently from the WASM demo.

## Upgrade and Freeze Boundary

Layer 5 freeze scope:

- all four on-chain programs
- IDLs consumed by the frontend
- keeper config schema
- operator CLI command surface relied on by runbooks

Post-freeze functional changes should be limited to audit-driven fixes. Non-functional work may continue in docs, runbooks, dashboards, and deployment config.

## Launch Assumptions

- flagship may remain paused while still deployed
- there is no public junior flow in v1
- there is no indexer in v1
- first issuance sizes remain demo-scale
- mainnet pause/unpause drills occur before external user issuance

## Non-v1 Items

Explicitly out of scope for this architecture:

- public junior deposits
- multi-issuer xStocks routing
- mobile app
- indexer-based portfolio analytics
- additional structured products beyond the three programs above

## Auditor Entry Points

Primary code entry points:

- `programs/halcyon_kernel/src/lib.rs`
- `programs/halcyon_sol_autocall/src/lib.rs`
- `programs/halcyon_il_protection/src/lib.rs`
- `programs/halcyon_flagship_autocall/src/lib.rs`
- `crates/halcyon_common/src/events.rs`
- `frontend/lib/halcyon.ts`

Primary operational entry points:

- `tools/halcyon_cli/src/main.rs`
- `keepers/*/src/main.rs`
- `docs/runbooks/mainnet_runbook.md`
- `ops/monitoring/README.md`
