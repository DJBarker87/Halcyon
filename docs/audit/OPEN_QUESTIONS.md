# Layer 5 Open Questions

This is the auditor/operator punch list for issues that are intentionally not resolved by code alone.

## Audit F1 follow-up (flagship hedge keeper)

The scaffold pass (`keepers/flagship_hedge_keeper/`) lands the crate,
KeeperRegistry authority check, 2D SPY/QQQ hedge composition using the
IWM projection, dual-staleness gates (age + spot drift), rebalance
trigger logic (cadence floor OR breach-of-1.5×-band), and `--dry-run`
behaviour. Live Jupiter routing + `prepare_hedge_swap` →
Jupiter swap → `record_hedge_trade` Flash-Fill v0 composition is
**deferred to a follow-up session** where it can reuse the SOL
Autocall hedge keeper's helpers.

**Flagship unpause predicate:** flagship issuance stays paused-public
until `flagship_hedge_keeper` has completed a successful devnet
rebalance cycle end-to-end (keeper writes, Jupiter executes, kernel
records, HedgeBookState reflects the trade). Unpausing flagship without
a working hedge keeper is exactly the configuration the audit flagged.

## Product activation

1. Should flagship issuance remain paused on mainnet at launch?
2. What exact legal sign-off is required before flagship moves from deployed+paused to live?
3. What minimum Jupiter route-depth evidence is required for flagship hedge activation?

## Oracle readiness

1. Which exact Pyth receiver accounts are the canonical mainnet feeds for SPY, QQQ, IWM, SOL, and USDC?
2. What fallback procedure applies if a single equity feed is degraded while SOL and IL remain healthy?

## Keeper operations

1. What is the final RPC failover policy per keeper?
2. Which operator owns each 24/7 alert?
3. Which keeper failures should page immediately versus create a daytime ticket?

## Capital and risk

1. Is the initial utilization alert threshold 85% or lower for launch week?
2. Are the launch risk caps intentionally lower than the current dev defaults?
3. Does launch policy sizing stay within the `$50–$500` smoke-test band for all three products?

## Frontend and user flow

1. Which wallet adapters are officially supported at launch?
2. Are there geoblocks, disclaimers, or KYC gates required on the transacting frontend before mainnet exposure?
3. Who signs off the final mainnet runtime config values loaded into the frontend environment?

## Audit and freeze

1. What is the exact freeze date for all four programs?
2. Which issues are acceptable to waive if auditors return only medium or low findings?
3. What evidence bundle is retained after launch: deploy signatures, multisig screenshots, alert screenshots, and first-smoke transactions?

## Post-submission optimization

**Quasar migration.** If flagship on-chain CU consumption becomes a
binding constraint, evaluate porting the kernel and four product
programs from Anchor 0.32.1 to Quasar
(<https://quasar-lang.com/docs>). Same-shaped macros (`#[program]`,
`#[account]`, `#[derive(Accounts)]`) as Anchor but with pointer-cast
account parsing, zero deserialization, and `#![no_std]` by default.

- **Expected win:** ~30% CU reduction in the dispatch/account-validation
  layer and ~30% program-rent reduction (smaller compiled binaries).
- **Migration cost:** estimated 3–5 focused days plus a fresh audit pass.
  Derive-macro diff is small; the load-bearing work is auditing every
  `#[account]` struct for `#[repr(C)]` alignment and field ordering
  (Borsh-tolerant → pointer-cast-strict transition).
- **Prerequisites:** flagship CU budget is actually binding in
  production (not before); `Box<[Option<FrozenPredictGrid>; 5]>` in
  `quote_c1_filter` and `Box<[Option<TripleCorrectionPre>; N_OBS]>` in
  `quote_c1_fast_trace` will need rework because `no_std` removes
  `alloc::boxed::Box`.
- **Do not pre-migrate.** Anchor 0.32.1 is battle-tested, IDL generation
  is automatic, and the CU headroom hasn't actually been measured against
  the 1.4M-per-tx cap yet. Optimize after traffic shape is known.
- **Pinocchio is the more aggressive alternative.** Bigger CU win, but
  manual account parsers everywhere — not worth the additional
  investment over Quasar unless CU is critically binding.
