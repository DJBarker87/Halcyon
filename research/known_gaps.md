# Known Gaps

## SOL Autocall keeper-fed POD-DEIM

### Demo posture accepted

- The demo reuses the `regime` keeper role to upload SOL Autocall reduced operators `P_red_v(σ)` / `P_red_u(σ)`. This avoids introducing a sixth keeper or another hot key during the sprint.
- The reduced-operator upload path is chunked across multiple transactions because the full `15×15` operator pair exceeds Solana's packet-size limit when sent in one instruction payload.

### Post-v1 hardening

- Split SOL reduced-operator writes into a dedicated keeper role and key, instead of reusing the regime keeper authority.
- Add a runbook for reduced-operator refresh cadence, resume/retry behavior, and recovery after partial uploads.
- Add monitoring and alerting for stale or incomplete `ReducedOperators` PDAs so pricing availability failures are visible before buyers hit them.
- Add key-management and rotation guidance for the keeper that can write reduced operators.
- Consider a more efficient wire format than chunked `Vec<i64>` uploads if the product catalog widens and per-product operator refresh becomes frequent.
