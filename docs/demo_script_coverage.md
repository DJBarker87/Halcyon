# Demo Script Coverage

Updated: 2026-04-24

## Exists In Product

| Beat | Product surface | Status |
| --- | --- | --- |
| Live note receipt with ticking fair value, coupon accrual, underlyings, on-chain mark tooltip, and explorer tx | `/demo` | Built |
| Collateral empty state, receipt drag affordance, and "Price this collateral" action | `/demo` | Built |
| One transaction with `preview_quote`, `price_note`, and `issue_loan` | Devnet tx `3rtbvWudzWGLya3dtKo9iRb2GzawbLpaSnqBBFq9TuLbpzNeyYRyGxUXfS4jivwAFR5bZLVjGUc7YwCfF1AXhy77` | Built and verified |
| Lending consumer program with named `price_note` and `issue_loan` instructions | `research/programs/halcyon_lending_consumer` on devnet `BSZABrfDG1vN3q7sejfebPFbqfqwVRu8gcjSukWEiXqF` | Built and deployed |
| Backtest Explorer with crisis shading, event table, and zero-failure counters | `/stress-tests` | Built |
| SOL autocall product view | `/sol-autocall` and `/demo` switcher | Existing, linked |
| LP protection product view | `/il-protection` and `/demo` switcher | Existing, linked |
| Final dashboard showing all three products | `/demo` close frame | Built |
| Judge mockUSDC faucet | `/faucet` and `tools/mock_usdc_faucet` | Built |

## Demo Data Notes

The draft script's "Buyback Events: 2,847" number was not present in the checked-in research output. The product now displays the actual checked-in summary instead:

| Scenario | Liquidations | Failures | Min coverage | Worst single day |
| --- | ---: | ---: | ---: | ---: |
| Primary | 452 | 0 | 133.2% | 107 |
| Stress | 708 | 0 | 124.6% | 73 |

## Live Devnet Facts

| Item | Value |
| --- | --- |
| mockUSDC mint | `5kFrfeo47etPpEk92eecACZboVnZrE4HsgxRUrg3TG7P` |
| mockUSDC faucet authority | `GvbrNomBk7ZzsrFs2QjD8aujrJy64mFVtmAXgQnhNqS` |
| 10m mockUSDC mint tx | `4C2zy4v5oNvJbbPFcC6BLSeXPL5KAqCTVTbB7AzV7AhAXr5i7L48EU3GQdbvsY6tuXMBYiDVVFHp88xKoPTM92ov` |
| One-tx borrow proof | `3rtbvWudzWGLya3dtKo9iRb2GzawbLpaSnqBBFq9TuLbpzNeyYRyGxUXfS4jivwAFR5bZLVjGUc7YwCfF1AXhy77` |
| Lending consumer deploy tx | `3b3324k9PXU2v3GJN3c9nm6PLoTrKbZVtUbkXyNpEdQJBnwz6r6JJ8kFYgbchHCmfrrpZh5cSBbjTMbtvNwbpT77` |
