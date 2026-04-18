# Colosseum Final

Minimal Halcyon export for the two retained Colosseum runtime paths:

- `IL Protection` on the Rust NIG European engine plus terminal settlement.
- `SOL Autocall` on the hedged autocall quote and replay stack.

Source-of-truth product docs carried into this repo:

- `halcyon_whitepaper_colosseum.md`
- `docs/il_hedge_challenger.md`
- `docs/halcyon_sol_autocall_v1_spec.md`

## Workspace

- `solmath-core`: fixed-point math dependency kept local.
- `crates/halcyon-quote`: trimmed product runtime crate.
- `samples/`: JSON payloads for both product CLIs.

## Run

```bash
make test
make il-hedge
make sol-autocall
```

The IL CLI writes a quote plus optional terminal settlement to an output JSON file.

The SOL autocall CLI writes quote diagnostics plus a replay result to an output JSON file.
