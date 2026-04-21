# Red Team Audit

Date: 2026-04-20
Repo: `colosseumfinal`
Mode: comprehensive red-team review with state-transition and data-flow tracing

## Scope and method

This audit covered:

- on-chain programs in `programs/`
- off-chain keepers in `keepers/`
- browser frontend in `frontend/`
- CI/CD and dependency posture
- local secret hygiene and git history spot checks

Validation performed:

- static code review of kernel, flagship, keepers, and frontend runtime paths
- `yarn audit --level high` at repo root
- `npm audit --audit-level=high` in `frontend/`
- `cargo audit`
- focused verification with `cargo test -p halcyon_flagship_autocall --lib`

Not performed:

- full localnet `anchor test`
- live RPC/browser exploitation
- production infrastructure review outside this repo

## Executive summary

The kernel itself is materially stronger than the outer layers. The main red-team findings are concentrated at the flagship product edge and the frontend trust boundary:

1. flagship monthly memory coupons can be skipped permanently if observation writes lag before autocall or maturity
2. flagship pause semantics are incomplete; pause does not freeze every state transition
3. frontend genesis-hash verification is advisory only; users can still preview and sign on a mismatched RPC
4. the shipped frontend dependency tree carries critical/high advisories, and current CI does not audit that tree
5. a release-only `mainnet-guards` invariant exists, but CI does not currently enforce it

## Attack surface map

### On-chain

- `programs/halcyon_kernel`: reserve, finalize, coupon, settlement, hedge, oracle writes, admin control
- `programs/halcyon_sol_autocall`
- `programs/halcyon_il_protection`
- `programs/halcyon_flagship_autocall`

### Off-chain

- `frontend/`: client-side Next.js app; no server routes found under `frontend/app`
- `keepers/observation_keeper`: schedules coupon/autocall/KI observations
- `keepers/hedge_keeper`: builds Jupiter hedge swaps
- `keepers/delta_keeper`: signs aggregate delta payloads and pins artifacts to Pinata
- `keepers/regime_keeper`: ingests CoinGecko daily market data
- `keepers/regression_keeper`: ingests Stooq CSV data
- `keepers/flagship_hedge_keeper`: scaffolded hedge keeper path

### External trust boundaries

- Solana RPC
- wallet adapter / browser wallet boundary
- Pyth price accounts
- Jupiter swap API
- Pinata API
- CoinGecko API
- Stooq data feed

## State transition map

### Kernel policy lifecycle

`reserve_and_issue` writes `PolicyHeader.status = Quoted` after reserve checks, premium transfers, and cap accounting.

- Evidence: `programs/halcyon_kernel/src/instructions/lifecycle/reserve_and_issue.rs:97`
- State commit: `programs/halcyon_kernel/src/instructions/lifecycle/reserve_and_issue.rs:279`

`finalize_policy` binds the product terms account and flips `Quoted -> Active`.

- Evidence: `programs/halcyon_kernel/src/instructions/lifecycle/finalize_policy.rs:54`
- State commit: `programs/halcyon_kernel/src/instructions/lifecycle/finalize_policy.rs:140`

`apply_settlement` pays out, releases reserved liability, and flips `Active -> Settled`.

- Evidence: `programs/halcyon_kernel/src/instructions/lifecycle/apply_settlement.rs:84`
- State commit: `programs/halcyon_kernel/src/instructions/lifecycle/apply_settlement.rs:169`

`pay_coupon` is non-terminal and operates only while the header remains `Active`.

### Flagship product lifecycle

`accept_quote` creates `FlagshipAutocallTerms` in `ProductStatus::Active` with:

- `next_coupon_index = 0`
- `next_autocall_index = 0`
- `missed_coupon_observations = 0`

Monthly coupon path:

- `record_coupon_observation` advances `next_coupon_index`
- if coupon barrier passes, it pays `coupon_due_with_memory_usdc(...)`
- otherwise it increments `missed_coupon_observations`

Evidence:

- `programs/halcyon_flagship_autocall/src/instructions/record_coupon_observation.rs:177`
- `programs/halcyon_flagship_autocall/src/instructions/record_coupon_observation.rs:214`
- `programs/halcyon_flagship_autocall/src/instructions/record_coupon_observation.rs:218`

Quarterly autocall path:

- `record_autocall_observation` reads Pyth, optionally pays a coupon, always advances `next_autocall_index`, and may autocall-settle the policy

Evidence:

- `programs/halcyon_flagship_autocall/src/instructions/record_autocall_observation.rs:188`
- `programs/halcyon_flagship_autocall/src/instructions/record_autocall_observation.rs:238`
- `programs/halcyon_flagship_autocall/src/instructions/record_autocall_observation.rs:255`

Knock-in path:

- `record_ki_event` can latch `ki_latched = true`

Evidence:

- `programs/halcyon_flagship_autocall/src/instructions/record_ki_event.rs:96`

Maturity path:

- `settle` may process one due coupon, then sets `ProductStatus::Settled`, then CPIs into kernel settlement

Evidence:

- `programs/halcyon_flagship_autocall/src/instructions/settle.rs:161`
- `programs/halcyon_flagship_autocall/src/instructions/settle.rs:227`
- `programs/halcyon_flagship_autocall/src/instructions/settle.rs:231`

### Hedge and oracle state

`write_aggregate_delta` verifies:

- keeper role
- non-empty publication CID
- Pyth publish-time freshness and monotonicity
- Ed25519 precompile proof over the canonical message
- strict sequence increment

Evidence:

- `programs/halcyon_kernel/src/instructions/oracle/write_aggregate_delta.rs:80`
- `programs/halcyon_kernel/src/instructions/oracle/write_aggregate_delta.rs:125`
- `programs/halcyon_kernel/src/instructions/oracle/write_aggregate_delta.rs:171`

`prepare_hedge_swap` enforces transaction shape:

- current instruction = prepare
- next instruction = Jupiter V6
- next-next instruction = matching `record_hedge_trade`
- no trailing instruction

Evidence:

- `programs/halcyon_kernel/src/instructions/lifecycle/prepare_hedge_swap.rs:351`
- `programs/halcyon_kernel/src/instructions/lifecycle/prepare_hedge_swap.rs:387`

## Data-flow map

### Issuance flow

1. User selects a cluster and RPC in the frontend runtime config.
2. Frontend fetches the RPC genesis hash and stores a local `genesisCheck` state.
3. `simulatePreview(...)` calls `fetchProtocolContext(...)`, builds a synthetic transaction, and runs `connection.simulateTransaction(...)`.
4. `buildBuyTransaction(...)` composes the product `accept_quote` instruction and lookup tables.
5. Wallet signs and sends the transaction.
6. Product program CPIs into kernel `reserve_and_issue`.
7. Product initializes terms and kernel `finalize_policy` flips the policy active.

Evidence:

- `frontend/lib/runtime-config.tsx:129`
- `frontend/lib/halcyon.ts:410`
- `frontend/lib/halcyon.ts:426`
- `frontend/lib/halcyon.ts:450`
- `frontend/components/issuance-page.tsx:230`
- `frontend/components/issuance-page.tsx:247`
- `programs/halcyon_kernel/src/instructions/lifecycle/reserve_and_issue.rs:97`
- `programs/halcyon_kernel/src/instructions/lifecycle/finalize_policy.rs:54`

### Observation and settlement flow

1. Off-chain keepers read Pyth and schedule clocks.
2. Coupon/autocall/KI observation instructions mutate flagship product state.
3. Coupon and settlement paths CPI into kernel money movement.
4. Kernel pays coupon from coupon vault or settlement from main vault to the buyer ATA.

### Risk/oracle publication flow

1. `regime_keeper` pulls CoinGecko history.
2. `regression_keeper` pulls Stooq CSV history.
3. `delta_keeper` computes deltas, signs the canonical message, pins the artifact to Pinata, then writes aggregate delta on-chain.
4. `hedge_keeper` prepares a kernel-approved hedge swap and executes Jupiter V6.

Evidence:

- `keepers/regime_keeper/src/config.rs:22`
- `keepers/regression_keeper/src/main.rs:62`
- `keepers/delta_keeper/src/main.rs:97`
- `keepers/delta_keeper/src/main.rs:155`
- `keepers/hedge_keeper/src/main.rs:87`

## Findings

### 1. High: flagship memory coupons can be skipped permanently before autocall or maturity

Impact:

- owed monthly coupons can become unrecoverable
- economic payout can diverge from product intent even when final settlement succeeds

Exploit path:

1. the observation keeper misses or withholds one or more monthly `record_coupon_observation` calls
2. `missed_coupon_observations` never increments because that counter advances only in the monthly handler
3. a quarterly autocall observation or maturity settlement arrives later
4. the code pays at most one coupon based on current state and then advances/settles
5. once the policy closes, skipped monthly coupons cannot be recovered

Why this is reachable:

- quarterly autocall only considers a coupon when `next_coupon_index == quarterly_coupon_index(expected_index)`
- maturity handles at most one due coupon
- the memory-coupon formula trusts `missed_coupon_observations`, not the schedule gap

Evidence:

- `programs/halcyon_flagship_autocall/src/instructions/record_coupon_observation.rs:214`
- `programs/halcyon_flagship_autocall/src/instructions/record_coupon_observation.rs:218`
- `programs/halcyon_flagship_autocall/src/instructions/record_autocall_observation.rs:188`
- `programs/halcyon_flagship_autocall/src/instructions/record_autocall_observation.rs:189`
- `programs/halcyon_flagship_autocall/src/instructions/record_autocall_observation.rs:238`
- `programs/halcyon_flagship_autocall/src/instructions/settle.rs:161`
- `programs/halcyon_flagship_autocall/src/instructions/settle.rs:219`
- `programs/halcyon_flagship_autocall/src/pricing.rs:213`

Recommended fix:

- make autocall and maturity paths settle all due monthly observations up to `now`, or
- fail closed unless `next_coupon_index` has already caught up to the required monthly schedule boundary

### 2. High: flagship pause does not freeze all state transitions

Impact:

- pause is not a full incident-stop control for flagship state
- an observation keeper can still advance lifecycle state during a pause window

Exploit path:

- `record_coupon_observation` checks `product_registry_entry.paused`
- `record_autocall_observation` does not
- `record_ki_event` does not

Practical effect:

- non-autocall quarterly observations can still advance `next_autocall_index`
- KI can still latch while the product is paused
- after unpause, the product can resume from mutated state rather than the frozen incident snapshot the operator expects

Evidence:

- paused gate present: `programs/halcyon_flagship_autocall/src/instructions/record_coupon_observation.rs:127`
- missing in autocall path: `programs/halcyon_flagship_autocall/src/instructions/record_autocall_observation.rs:134`
- state mutation in autocall path: `programs/halcyon_flagship_autocall/src/instructions/record_autocall_observation.rs:238`
- missing in KI path: `programs/halcyon_flagship_autocall/src/instructions/record_ki_event.rs:61`
- KI latch: `programs/halcyon_flagship_autocall/src/instructions/record_ki_event.rs:97`

Recommended fix:

- add the same per-product pause gate to `record_autocall_observation` and `record_ki_event`
- add regression tests that assert no flagship state changes while paused

### 3. High: frontend genesis-hash verification is advisory only

Impact:

- users can still preview, connect wallets, compose transactions, and sign against a mismatched RPC
- cluster integrity is displayed as a warning, not enforced as a hard gate

Exploit path:

1. frontend is configured with a hostile or incorrect RPC endpoint
2. runtime config detects genesis mismatch and sets `genesisCheck.status = "error"`
3. providers still create a live `ConnectionProvider` and wallet provider
4. issuance page still allows preview and issue actions

This is enough for:

- misleading previews from the wrong cluster
- transaction composition against a hostile RPC
- user signing on an environment the app has already determined is wrong

Evidence:

- mismatch detection only updates state: `frontend/lib/runtime-config.tsx:145`
- providers remain live: `frontend/app/providers.tsx:28`
- error is shown as banner copy: `frontend/components/app-shell.tsx:209`
- issuance page does not gate on `genesisCheck`: `frontend/components/issuance-page.tsx:196`
- preview gate ignores genesis status: `frontend/components/issuance-page.tsx:214`
- preview still executes: `frontend/components/issuance-page.tsx:230`
- issue path still executes: `frontend/components/issuance-page.tsx:247`

Recommended fix:

- fail closed when `genesisCheck.status == "error"`
- disable preview, issue, portfolio, and wallet connect surfaces until the runtime cluster verifies
- add browser tests that prove mismatch blocks signing paths

### 4. Medium-High: frontend dependency tree has critical/high advisories, and CI misses it

Impact:

- the browser wallet layer currently inherits vulnerable transitive packages
- CI gives a false sense of coverage because it audits only the repo-root package set

Observed results:

- repo-root `yarn audit --level high`: no high findings
- `frontend/npm audit --audit-level=high`: 38 findings, including critical `protobufjs`, plus `elliptic`, `lodash`, and `bigint-buffer`

Code context:

- frontend depends on `@solana/wallet-adapter-wallets`
- CI runs `yarn install` and `yarn audit` only at repo root

Evidence:

- wallet adapter dependency: `frontend/package.json:18`
- CI root install: `.github/workflows/ci.yml:69`
- CI root audit: `.github/workflows/ci.yml:70`

Recommended fix:

- audit `frontend/package-lock.json` in CI directly
- reduce wallet adapter surface to the exact adapters you ship
- pin audited versions instead of leaving wide `^` ranges on the wallet stack

### 5. Medium: release CI does not enforce the `mainnet-guards` build invariant

Impact:

- a non-localnet release artifact can be built without the explicit guard that rejects known test-only product IDs

Why this matters:

- the kernel advertises `mainnet-guards` as a release-only invariant
- the feature defaults off
- CI never runs the check that enforces it

Evidence:

- feature defaults off: `programs/halcyon_kernel/Cargo.toml:12`
- guard feature exists: `programs/halcyon_kernel/Cargo.toml:25`
- documented release check: `Makefile:126`
- command exists: `Makefile:129`
- CI does not invoke it: `.github/workflows/ci.yml:22`

Recommended fix:

- add `make mainnet-guards-check` to release CI
- refuse release packaging unless the kernel build includes `--features mainnet-guards`

## Additional observations

- No hardcoded secrets were found in tracked files during spot checks.
- `.env`, `.env.*`, keypairs, and PEM-style files are gitignored.
- A local `.env` exists in the working tree and appears to be used only for expected operational variables.
- CI action pins are good: GitHub Actions are SHA-pinned and the Solana CLI tarball hash is verified.
- The frontend appears to be browser-only. No API routes or inbound webhook handlers were found in repo code.
- Kernel hedge and oracle paths are notably stronger than average:
  - Jupiter is allowlisted and transaction shape is constrained.
  - aggregate delta writes require both fresh publish times and Ed25519-precompile verification.
- `cargo audit` still reports upstream Solana/Anchor transitive advisory debt, already documented in `security/cargo_audit_waivers.md`.

## Verification notes

- `cargo test -p halcyon_flagship_autocall --lib`: passed, 8/8 tests
- a separate `cargo test -p flagship_hedge_keeper` was started during review but intentionally aborted after lock contention because it was not needed for the confirmed findings above

## Priority order

1. fix flagship coupon catch-up semantics before unpausing flagship
2. make flagship pause a true freeze across all observation handlers
3. fail closed on frontend genesis mismatch
4. audit and trim the frontend wallet dependency tree in CI
5. enforce `mainnet-guards` in release automation
