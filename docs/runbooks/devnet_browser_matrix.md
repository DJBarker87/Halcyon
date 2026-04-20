# Devnet Browser Test Matrix

This is the manual Layer 5 browser-driven checklist required by the build doc.

## Frontend baseline

For each page:

- `/flagship`
- `/sol-autocall`
- `/il-protection`
- `/portfolio`
- `/vault`

Verify on:

- desktop width (~1280 px)
- tablet width (~768 px)
- mobile width (~375 px)

## Runtime config

1. Load devnet RPC and deployed program IDs.
2. Fill all required Pyth receiver accounts.
3. Reload the page and confirm values persist.

## Wallet session

1. Connect wallet.
2. Disconnect wallet.
3. Reconnect wallet.
4. Confirm the UI returns to a healthy state on every page.

## Issuance

Run one successful browser-driven issuance per product:

- flagship
- SOL Autocall
- IL Protection

For each product:

1. preview quote
2. submit issuance
3. confirm transaction signature
4. confirm portfolio reflects the policy

## Rejection paths

For each product:

1. preview quote
2. tighten slippage or manipulate bounds enough to force rejection
3. submit issuance
4. confirm the error is surfaced and no policy is created

## Vault and portfolio

After issuance:

1. confirm portfolio lists policies under the correct product
2. confirm vault page updates reserved liability and product summary

## Signoff

Record:

- date
- operator
- wallet used
- RPC endpoint
- transaction signatures
- screenshots for any notable failure or retry condition
