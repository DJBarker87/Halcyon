# SOL Midlife Matrix Verification

The SOL autocall midlife pricer uses keeper-uploaded transition matrices to keep
the lending-value preview below the Solana compute limit. The matrix PDA stores
two SHA-256 commitments so a third party can independently regenerate and verify
the uploaded artefact.

## On-Chain Commitments

`SolAutocallMidlifeMatrices` stores:

- `construction_inputs_sha256`: SHA-256 over the deterministic matrix-builder
  inputs.
- `matrix_values_sha256`: SHA-256 over the uploaded row-major matrix values,
  bound to `construction_inputs_sha256`.

The program recomputes both hashes on every `write_midlife_matrices` chunk and
checks both hashes again before `preview_lending_value` accepts the matrix.

## Construction-Input Hash

Domain:

```text
halcyon:sol-autocall:midlife-matrix-inputs:v1
```

All numeric fields are little-endian. The hash covers:

1. matrix account version
2. SOL autocall engine version
3. pricing sigma
4. account `n_states` and `cos_terms`
5. compiled matrix shape constants
6. observation schedule constants
7. knock-in/autocall barriers and log barriers
8. NIG training alpha, beta, and reference step
9. source vault-sigma and regime slots
10. uploaded step count and each `step_days_s6`

## Matrix-Values Hash

Domain:

```text
halcyon:sol-autocall:midlife-matrix-values:v1
```

The hash covers:

1. `construction_inputs_sha256`
2. uploaded step count
3. each step's `step_days_s6` and uploaded length
4. flattened row-major matrix values as signed i64 LE

An auditor can read the matrix PDA, recompute `construction_inputs_sha256`,
regenerate the exact matrix for each committed step, compare the row-major values
against the PDA, and then recompute `matrix_values_sha256`.
