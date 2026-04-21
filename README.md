# Halcyon

**Quant math that fits on-chain.**

On-chain structured products on Solana. Every coupon, every delta, every risk surface computed by a Rust program — not by an off-chain pricing service.

## The flagship product

**Equity Autocall** — an 18-month worst-of-3 autocallable note on SPY, QQQ, and IWM (tokenized via Backed Finance). You buy it in USDC; the program pays you a monthly coupon targeting roughly 15% annualised while the worst performer stays above its entry level; the note calls you out early every quarter if the basket is healthy; at maturity you get your principal back unless the worst performer has fallen past a 20% knock-in barrier.

What's new: **the coupon you see is the coupon you get, and anyone can re-run the pricer.** No market-maker quote feed. No custodial oracle. No trust.

Two companion products live in the same kernel:

- **IL Protection** — 30-day impermanent-loss cover for Raydium SOL/USDC LPs, priced with a NIG-distribution model.
- **SOL Autocall** — 16-day principal-backed note on SOL with 8 observations.

All three share one underwriting vault and one kernel program.

## Why this is different

Every previous on-chain structured-products protocol — Ribbon, Cega, Friktion — died from the same failure mode: the pricing lived off-chain, buyers had to trust a quote oracle or a market-maker intent, and the mechanism collapsed the moment that trust evaporated. Halcyon's pricing runs *inside the Solana program* in fixed-point integer arithmetic, deterministic across every validator. You can verify any quote by calling `simulateTransaction` against the program yourself.

See `halcyon_whitepaper_v9.md` for the model, `integration_architecture.md` for how it maps to programs and accounts, and `ARCHITECTURE.md` for the kernel shape.

## Repo layout

- `solmath-core/` — fixed-point math library (`ln`, `exp`, `sqrt`, Bessel K₁, bivariate normal CDF, NIG density).
- `crates/halcyon_flagship_quote/`, `halcyon_il_quote/`, `halcyon_sol_autocall_quote/` — per-product pricing crates.
- `programs/halcyon_kernel/` — shared vault, policy, and fee-ledger program.
- `programs/halcyon_flagship_autocall/`, `halcyon_il_protection/`, `halcyon_sol_autocall/` — product programs.
- `keepers/` — off-chain processes that write oracle / regression / aggregate-delta state and execute hedges.
- `frontend/` — Next.js app; the surface Colosseum judges see.
- `app/` — earlier WASM demo; useful as a wallet-free showcase.
- `samples/` — JSON payloads for the CLI smoke tests.
- `docs/` — audit notes, runbooks, product specs.

## Run

```bash
make test              # pricing-crate smoke tests
make il-hedge          # run the IL CLI against a sample
make sol-autocall      # run the SOL Autocall CLI against a sample
make frontend-build    # build the Next.js app
make frontend-e2e      # run Playwright tests
```

The IL and SOL Autocall CLIs each emit a JSON diagnostics file that includes the quote, the settlement path, and a replay result. These are the smallest end-to-end exercises of the on-chain pricing that don't require a running validator.

## Status

- Devnet: deployed, demoable.
- Mainnet: programs are audit-frozen; flagship stays paused to public issuance until the hedge keeper completes a successful devnet rebalance cycle end-to-end (see `docs/audit/OPEN_QUESTIONS.md`).
