# Midlife Pricer Reports

**Generated:** 2026-04-24

This index collects the product-level midlife pricer reports for the three Halcyon products that need live collateral marks.

| Product | Report | On-chain pricing shape | Collateral policy |
|---|---|---|---|
| Flagship SPY/QQQ/IWM worst-of autocall | `product_economics/flagship_midlife_pricer_report.md` | Checkpointed C1-filter dynamic program across multiple instructions | NAV less haircut, capped by KI level |
| SOL autocall | `product_economics/sol_autocall_midlife_pricer_report.md` | One preview instruction using keeper-uploaded, SHA-256 verified 9-state Markov matrices | NAV less haircut, capped by KI level |
| SOL/USDC IL protection | `product_economics/il_protection_midlife_pricer_report.md` | One preview instruction using shifted NIG European IL premium | 80% of current intrinsic payout only |

## Common Pattern

All three products expose a live mark from on-chain state and fresh oracle inputs. None of the lending paths accept an issuer-signed mark as the source of truth.

| Layer | Flagship | SOL Autocall | IL Protection |
|---|---|---|---|
| Live oracle input | SPY, QQQ, IWM | SOL | SOL and USDC |
| Live volatility input | Vault sigma and regime | Vault sigma and regime | Vault sigma and regime |
| Product state | Policy and autocall terms | Policy and autocall terms | Policy and IL terms |
| On-chain valuation output | NAV and lending value | NAV and lending value | NAV and intrinsic-only lending value |
| Heavy compute strategy | Checkpoint account | Keeper matrix account | Direct one-shot pricing |
| Verification artefact | Midlife parity fixtures and checkpoint identity tests | Matrix construction/value SHA-256 commitments | Direct host and integration tests |

## Testing Entry Points

| Coverage | Command / file |
|---|---|
| Flagship midlife parity | `tests/integration/midlife_parity.spec.ts` |
| Flagship real product flow | `tests/integration/real_products.spec.ts` |
| SOL and IL midlife integration | `tests/integration/sol_il_midlife.spec.ts` |
| SOL midlife host tests | `cargo test -p halcyon_sol_autocall_quote midlife -- --nocapture` |
| IL midlife host tests | `cargo test -p halcyon_il_quote midlife -- --nocapture` |
| Flagship midlife host tests | `cargo test -p halcyon_flagship_quote midlife -- --nocapture` |

## Report Maintenance

Update these reports whenever one of the following changes:

1. Collateral haircut or advance-rate policy.
2. Matrix/checkpoint account layout.
3. Product payoff terms.
4. NIG or C1-filter engine constants.
5. Integration-test CU results.
6. Backtest economics that change the risk interpretation of the live mark.
