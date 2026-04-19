# Cargo Audit Waivers

Last reviewed: 2026-04-19  
Next review due: 2026-05-19

These waivers are stage-scoped to L0-L2 and exist because the Solana 2.3.0 / Anchor 0.32.1 stack still pins transitive crypto crates the repo does not control directly. They are not silent accepts: CI still runs `cargo audit`, and the ignore list is limited to the two advisories that currently hard-fail the build.

## Hard-fail waivers in CI

1. `RUSTSEC-2024-0344` (`curve25519-dalek` 3.2.0)
   - Source: transitive through `ed25519-dalek` 1.0.1 inside the Solana 2.3.x graph.
   - Why waived now: repo-local crates do not call the affected subtraction internals directly; the vulnerable version is anchored under the upstream Solana/Anchor release train.
   - Local mitigation: no custom signing or scalar arithmetic wraps this dependency; monitor Solana/Anchor upgrades and drop the waiver as soon as upstream moves to `curve25519-dalek >= 4.1.3`.

2. `RUSTSEC-2022-0093` (`ed25519-dalek` 1.0.1)
   - Source: transitive through Solana signer/keypair crates.
   - Why waived now: the issue is upstream in the signing stack and cannot be patched repo-locally without forking the Solana dependency graph.
   - Local mitigation: keep all signing on standard Solana SDK paths; no repo-local alternate verifier or signature-oracle surface is introduced.

## Tracked warnings not ignored in CI

1. `RUSTSEC-2026-0097` (`rand` 0.7.3 / 0.9.2)
   - Current status: warning only.
   - Action: re-evaluate once Solana and `reqwest` transitive updates land; not suppressing in CI today.

2. `RUSTSEC-2024-0375`, `RUSTSEC-2025-0141`, `RUSTSEC-2024-0388`, `RUSTSEC-2025-0161`, `RUSTSEC-2025-0119`, `RUSTSEC-2024-0436`, `RUSTSEC-2021-0145`
   - Current status: unmaintained/unsound warnings in upstream transitive crates.
   - Action: tracked as supply-chain debt. None justify a repo-local fork at L0-L2, but they must be re-reviewed before widening keeper or CLI operational scope.
