# Layer 5 Test Coverage Snapshot

This snapshot is for audit kickoff and launch readiness.

## Automated coverage in repo

### Kernel and Rust surfaces

- `tests/kernel/kernel.spec.ts`
  - protocol initialization
  - product registration
  - deposit/withdraw gating
  - issuance/finalization path
  - settlement path
  - paused-state rejection
  - ALT-backed issuance path
- `cargo test -p halcyon_sol_autocall_quote --test smoke`
- `cargo test -p halcyon_il_quote --test smoke`
- `cargo test -p halcyon_flagship_autocall --lib`
- workspace `cargo check`

### Frontend

- `cd frontend && npm run build`
- `cd frontend && npm run test:e2e`
  - app shell/navigation rendering
  - issuance-page missing-config and runtime-config persistence
  - burner-wallet connect/disconnect smoke on localnet UI
  - portfolio and vault empty/disconnected states

## Manual devnet coverage required before launch

These are Layer 5 launch blockers until executed and recorded:

1. Browser-driven issuance on devnet for:
   - flagship
   - SOL Autocall
   - IL Protection
2. Slippage rejection path per product
3. Wallet disconnect recovery during a live browser session
4. Keeper heartbeat verification with real devnet config
5. Pause/unpause circuit-breaker drill
6. Mainnet smoke checklist dry run on devnet multisig

## Known current gaps

- No fully automated browser wallet-extension test against real devnet.
- No automated mainnet smoke suite; this remains an operator runbook.
- Monitoring alerts are templated, not validated against a live metrics backend in this repo.
- Frontend e2e smoke does not prove successful issuance; it proves the Layer 5 UI state and wallet/session handling.

## Release gate recommendation

Treat launch as blocked until:

- all automated checks above are green,
- the manual devnet matrix is signed off,
- and any critical or high audit findings are fixed or explicitly accepted with written justification.
