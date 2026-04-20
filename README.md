# Colosseum Final

Minimal Halcyon export for the two retained Colosseum runtime paths:

- `IL Protection` on the Rust NIG European engine plus terminal settlement.
- `SOL Autocall` on the hedged autocall quote and replay stack.

Source-of-truth product docs carried into this repo:

- `halcyon_whitepaper_colosseum.md`
- `docs/halcyon_flagship_autocall_v1_spec.md`
- `docs/il_hedge_challenger.md`
- `docs/halcyon_sol_autocall_v1_spec.md`

## Workspace

- `solmath-core`: fixed-point math dependency kept local.
- `crates/halcyon-quote`: trimmed product runtime crate.
- `samples/`: JSON payloads for both product CLIs.
- `frontend/`: Layer 5 production frontend copy wired to Anchor IDLs and wallet adapter.
- `app/`: legacy WASM demo kept for presentation-only use.

## Layer 5 artifacts

- `ARCHITECTURE.md`
- `THREAT_MODEL.md`
- `docs/audit/`
- `docs/runbooks/`
- `ops/monitoring/`
- `config/examples/`

## Run

```bash
make test
make il-hedge
make sol-autocall
make frontend-build
make frontend-e2e
```

The IL CLI writes a quote plus optional terminal settlement to an output JSON file.

The SOL autocall CLI writes quote diagnostics plus a replay result to an output JSON file.
