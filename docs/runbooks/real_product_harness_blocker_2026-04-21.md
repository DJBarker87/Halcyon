# Real-Product Harness Blocker: SOL Autocall Loader Failure

Date: 2026-04-21

## Status

The new real-product integration harness is currently blocked. The localnet run cannot proceed until `halcyon_sol_autocall` is rebuilt through the planned `build.rs` POD-DEIM precomputation pipeline.

This note records the diagnostic findings from the attempted harness bring-up. It is intentionally a note only. No pricing-code, cache-structure, or section-stripping workaround should be applied as part of this blocker response.

## Reproduction

The blocker reproduces while bringing up the integration harness with:

```bash
bash tests/integration/run_integration.sh
```

That script builds the integration-test artifacts, starts `solana-test-validator`, and then relies on Anchor deployment of the real programs.

## Observed Failure

`halcyon_kernel`, `halcyon_flagship_autocall`, and `halcyon_il_protection` deploy, but `halcyon_sol_autocall` fails during deployment with a Solana loader ELF parse error:

```text
Error: ELF error: ELF error: Failed to parse ELF file:
Section or symbol name '.bss._ZN26halcyo' is longer than '16' bytes
```

Because the validator never finishes deploying the full program set, `tests/integration/real_products.spec.ts` cannot run against the intended topology.

## Diagnostic Findings

Inspection of `target/deploy/halcyon_sol_autocall.so` showed long `.bss.*` sections that are not present in the other deployed program artifacts. Relevant section names included:

```text
.bss._ZN26halcyon_sol_autocall_quote15autocall_v2_e1115E11_QUOTE_CACHE17ha409c4ed51b60d66E
.bss._ZN14matrixmultiply4gemm8MASK_BUF28_$u7b$$u7b$closure$u7d$$u7d$3VAL17h1c7385d24a2726f8E
```

The deployed artifact path was:

```text
target/deploy/halcyon_sol_autocall.so
```

The pricing crate path implicated by the symbol names was:

```text
crates/halcyon_sol_autocall_quote/src/autocall_v2_e11.rs
```

## Current Assessment

This is a Solana loader constraint issue, not a justification for a quick cache-removal workaround.

The correct fix remains the previously identified `build.rs` POD-DEIM precomputation work for SOL autocall. That work is expected to eliminate the runtime cache-backed path in a deliberate way and make this class of deploy failure impossible, rather than masking it.

## Decision

For this integration-harness branch:

- Do not strip ELF sections.
- Do not remove caches.
- Do not refactor pricing code as a deploy workaround.
- Do not claim the real-product integration harness is runnable yet.

## Consequence For Harness Delivery

Until the SOL autocall program is rebuilt through the planned `build.rs` pipeline and can deploy cleanly to localnet:

- the full four-program topology cannot be stood up,
- the real-product integration suite cannot complete,
- and the required negative-control demonstrations cannot be treated as executed.

The harness work should therefore be treated as blocked on the SOL autocall build pipeline milestone, not as green or partially validated.
