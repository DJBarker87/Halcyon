# Layer 5 Open Questions

This is the auditor/operator punch list for issues that are intentionally not resolved by code alone.

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
