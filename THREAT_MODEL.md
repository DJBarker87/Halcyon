# Halcyon Threat Model

This document captures the Layer 5 launch threat model for code review, audit prep, and operations.

## Security Goals

The system should preserve these invariants:

1. User funds only move according to product rules and kernel accounting.
2. Product issuance cannot exceed protocol or per-product risk caps.
3. Stale or manipulated external data should fail issuance or settlement rather than silently produce bad state.
4. Keeper compromise should be limited by role checks, protocol caps, and pause controls.
5. Mainnet admin authority should not be a single-key failure.

## Protected Assets

- user USDC and escrowed principal
- vault capital and junior seed capital
- product terms and policy lifecycle state
- keeper authorities
- admin authority
- protocol ALT registry entries
- oracle freshness assumptions

## Threat Actors

- external user trying to bypass slippage or capacity checks
- compromised keeper key
- compromised admin key
- stale or degraded RPC provider
- stale or malformed oracle data
- MEV or market participant exploiting hedge or issuance timing
- frontend/runtime misconfiguration by operator
- dependency or infrastructure compromise in the web or keeper stack

## Trust Boundaries

### On-chain boundary

Programs trust only:

- their own state
- kernel/product CPI constraints
- current signer set
- oracle reads that satisfy freshness and identity checks

### Off-chain boundary

Keepers and frontend trust:

- RPC responses
- local config
- external APIs such as Jupiter or historical data providers

These must be treated as failure-prone and monitored accordingly.

## Major Threats and Mitigations

### 1. Admin key compromise

Impact:

- protocol parameter corruption
- malicious keeper rotation
- unauthorized product activation or pausing

Mitigations:

- transfer `ProtocolConfig.admin` to multisig before mainnet issuance
- devnet rehearsal for every admin path through the chosen multisig UX
- documented mainnet ceremony and evidence capture
- no single operator key should retain live mainnet admin power

Residual risk:

- multisig signer collusion or poor hardware hygiene

### 2. Keeper key compromise

Impact:

- malformed oracle writes
- bad hedge execution attempts
- stale/noisy state updates

Mitigations:

- role-separated keeper authorities in `KeeperRegistry`
- protocol caps such as `hedge_max_slippage_bps_cap`
- pause flags on issuance and settlement
- alerting on keeper error loops and stale heartbeats
- fresh mainnet keypairs per role

Residual risk:

- a compromised keeper can still cause denial-of-service until rotated out

### 3. Oracle staleness or feed mismatch

Impact:

- issuance on stale prices
- settlement on stale prices
- incorrect sigma/regime/regression use

Mitigations:

- staleness caps in `ProtocolConfig`
- explicit Pyth feed identity checks in product/oracle code
- monitoring on feed age vs cap
- product preview and settlement fail closed when feeds are stale

Residual risk:

- upstream market microstructure stress can still make valid fresh quotes economically poor

### 4. Quote drift between preview and accept

Impact:

- user signs at one perceived price and executes at another

Mitigations:

- preview slot bounds
- entry-price deviation bounds
- expiry drift bounds
- max premium and min liability bounds
- user-editable slippage with conservative defaults

Residual risk:

- users can choose loose bounds; UI should keep defaults tight

### 5. ALT registry misconfiguration

Impact:

- issuance transactions fail to compile or resolve
- wrong lookup tables could break the transacting surface

Mitigations:

- product-specific ALT registry on kernel side
- frontend treats missing or unreadable tables as hard errors
- mainnet ceremony includes ALT registration validation

Residual risk:

- operational error during deploy if tables are not registered before frontend use

### 6. Hedge execution path abuse

Impact:

- sleeve capital leakage
- poor fills from unsafe routes

Mitigations:

- hedge slippage cap in protocol config
- Jupiter program allowlist in hedge keeper config
- quote sanity checks in hedge keeper
- alerting on failed hedge preparation or trade recording

Residual risk:

- external liquidity deterioration still creates economic loss even with bounded slippage

### 7. Capacity exhaustion / insolvency risk

Impact:

- protocol over-issues liabilities relative to capital

Mitigations:

- kernel-side utilization cap
- per-product and global reserved-liability caps
- product registry reservation tracking
- monitoring alerts at 85% utilization before hard caps are hit

Residual risk:

- model mismatch or clustered losses remain economic risk even if accounting is correct

### 8. Frontend runtime misconfiguration

Impact:

- wrong cluster/program/oracle wiring
- user signs transactions against incorrect accounts

Mitigations:

- runtime config is explicit and visible in UI
- missing required values gate user flows
- previews fail visibly rather than producing local client-side values
- `.env.example` and config examples document the expected surfaces

Residual risk:

- local storage can still be manually corrupted by the operator

### 9. Browser wallet UX failures

Impact:

- stuck session
- accidental disconnect during issuance

Mitigations:

- wallet-adapter surface
- Layer 5 browser smoke coverage
- explicit manual devnet wallet disconnect recovery checklist

Residual risk:

- extension-specific issues remain outside smart-contract control

### 10. Dependency and supply-chain risk

Impact:

- compromised npm or cargo dependency
- exploitable frontend dependency vulnerability

Mitigations:

- explicit audit snapshot
- cargo audit waivers tracked in `security/`
- frontend dependency install is isolated to `frontend/`
- production launch should pin lockfiles and review critical vulnerabilities

Residual risk:

- `npm audit` currently reports vulnerabilities in the wallet stack transitive tree and requires ongoing review

### 11. Flagship aggregate-delta misunderstanding or misstatement

Impact:

- operators or reviewers treat flagship hedge inputs as a heuristic estimate when they are actually derived from the analytical gradient path
- disclosures understate the off-chain keeper dependency or overstate what has been formally validated

Mitigations:

- the flagship delta keeper uses the analytical `quote_c1_filter_with_delta_live` path, not Monte Carlo and not a placeholder estimate
- the flagship issuance quote path `corrected_coupon_bps_s6 -> quote_c1_filter` is fully fixed-point deterministic on-chain; the shipped Solana build does not rely on `f64`/`f32` in that path
- the core `triangle_probability_with_grad` primitive is Stein-validated against the shipped expectation path
- the higher-level pricer path has finite-difference sanity coverage in the quote crate
- `AggregateDelta` stores a Merkle commitment so auditors can recompute per-note deltas from product terms plus the recorded Pyth spot snapshot
- the flagship product spec and frontend issuance notes disclose the methodology explicitly

Residual risk:

- the aggregate delta is still computed off-chain, so wrapper bugs, stale config, or bad operator procedures can produce wrong hedge inputs even when the core gradient primitive is validated

## Severity Map

- Critical: direct unauthorized fund movement or protocol takeover
- High: material loss path, keeper or oracle corruption with unsafe settlement, or sustained inability to pause
- Medium: denial-of-service on issuance/settlement, operator misconfiguration, monitoring blind spots
- Low: observability gaps, documentation drift, non-fund UX issues

## Pre-Launch Controls

Before mainnet issuance opens:

1. Multisig controls `ProtocolConfig.admin`.
2. Keeper keys are rotated from devnet to fresh mainnet keys.
3. Product ALT registries are populated and verified.
4. Pause/unpause drill is executed on the live deployment.
5. Monitoring and alerts are live for RPC, feed age, keeper heartbeat, utilization, and settlement failures.
6. Frontend runtime values for mainnet are reviewed by two operators.

## Launch Residual Risks

Residual risks that remain even after mitigations:

- flagship legal and liquidity readiness may keep the product paused
- off-chain keepers remain an operational dependency
- flagship aggregate delta is analytically derived and auditable, but it is still an off-chain control surface rather than an on-chain invariant
- no indexer means large account scans depend on raw RPC performance
- browser wallet automation is limited; some wallet scenarios remain manual

Those risks are acceptable only if they are visible, monitored, and bounded by pause controls.
