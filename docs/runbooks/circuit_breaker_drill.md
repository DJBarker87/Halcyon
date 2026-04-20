# Circuit Breaker Drill

Run this on devnet before mainnet, then once on mainnet immediately after deploy.

## Objective

Prove the protocol fails closed when issuance is paused and resumes correctly after unpause.

## Steps

1. Prepare a low-notional issuance candidate in the frontend or CLI.
2. Pause issuance globally through the admin path.
3. Attempt issuance and record the rejection.
4. Unpause issuance.
5. Repeat the same issuance flow.
6. Confirm preview succeeds and the signed transaction lands.

## Expected outcomes

- paused state rejects issuance
- rejection is visible in the frontend or CLI without ambiguous partial state
- unpaused state allows issuance
- no orphaned `PolicyHeader` or reservation remains from the rejected path

## Evidence to capture

- pause transaction signature
- rejection screenshot or CLI output
- unpause transaction signature
- successful issuance transaction signature

## Follow-up checks

- `status` output remains consistent
- portfolio page shows only the successful issuance
- monitoring receives no unexpected settlement or utilization alerts
