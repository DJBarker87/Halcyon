# Verifying flagship `AggregateDelta` end-to-end

**Audience:** external auditors, operators, anyone who wants to
independently confirm that an on-chain `AggregateDelta` snapshot was
produced by the registered delta keeper and attests to the per-note
breakdown it commits to.

This procedure resolves audit findings 4a (Merkle publication) and 4b
(keeper signature). Every claim the code makes about provenance can be
checked from scratch without trusting the keeper binary, the RPC
endpoint, or the IPFS gateway individually.

## What the kernel stores

`AggregateDelta` (v2) carries three audit-facing fields in addition to
the deltas and spot snapshot:

| field | source | purpose |
|---|---|---|
| `pyth_publish_times` | Pyth receiver `publish_time` for each of SPY, QQQ, IWM | Finding 2 — keeper cannot stamp a stale feed as fresh; kernel enforces per-feed monotonicity between writes |
| `sequence` | increments by 1 on every successful write | Finding 4b — part of the signed canonical message; defeats replay of an earlier valid (args, signature) pair |
| `keeper_signature` | Ed25519 signature over `encode_aggregate_delta_message(...)` | Finding 4b — verified on-chain via the Ed25519 precompile; stored here so an external reader can replay the verification offline |
| `publication_cid` | IPFS CID of the canonical per-note JSON artifact | Finding 4a — points at the off-chain content whose Merkle root equals the on-chain `merkle_root` |

## Canonical signed-message layout

The delta keeper signs and the kernel verifies exactly these 147 bytes:

```
offset  len  field
 0      27   "halcyon-aggregate-delta-v1\n"      (domain-separation tag)
27      32   merkle_root                          (SHA-256 over per-note leaves)
59      24   pyth_publish_times[SPY,QQQ,IWM]      (3 × i64 little-endian)
83      24   spot_snapshot_s6[SPY,QQQ,IWM]        (3 × i64 LE, SCALE_6)
107      8   sequence                             (u64 LE)
115     32   product_program_id                   (Pubkey)
```

Canonical encoder in Rust: `crates/halcyon_common/src/aggregate_delta_signing.rs`.
Canonical encoder in Python: `research/tools/verify_aggregate_delta.py`
(re-implemented from the byte layout so verification does not depend on
the Rust workspace).

## Verification steps

1. **Fetch the on-chain account.** Read the `AggregateDelta` PDA for the
   flagship product and the `KeeperRegistry`. The registered delta
   keeper is `KeeperRegistry.delta`.

2. **Re-encode the canonical bytes.** Using the values stored on-chain —
   `merkle_root`, `pyth_publish_times`, `spot_*`, `sequence`,
   `product_program_id` — produce the 147-byte canonical message.

3. **Verify the signature.** Using standard Ed25519 (RFC 8032), verify
   `AggregateDelta.keeper_signature` over the canonical bytes against
   the `KeeperRegistry.delta` pubkey. Any standard cryptography library
   (pynacl, libsodium, ed25519-dalek, Java's Ed25519 provider) produces
   an identical result.

4. **Fetch the IPFS artifact.** Fetch the content at
   `AggregateDelta.publication_cid` via any IPFS gateway. Pinata's
   `gateway.pinata.cloud` is the production path; public gateways such
   as `cloudflare-ipfs.com` or `ipfs.io` also resolve pinned content.

5. **Re-compute the Merkle root.** For each note in the artifact,
   recompute the leaf:
   ```
   leaf = SHA-256("flagship-delta-leaf" || policy_pubkey ||
                  delta_spy_s6 LE || delta_qqq_s6 LE || delta_iwm_s6 LE)
   ```
   Combine pairwise:
   ```
   node = SHA-256("flagship-delta-node" || left || right)
   ```
   (duplicate the tail leaf if the level has odd length). The final
   node must equal `AggregateDelta.merkle_root`.

6. **(Optional) Spot-check a single note.** Pull the note's
   `ProductTerms` account from the same slot, recompute its delta using
   the open-source SolMath + flagship-quote gradient path, and confirm
   the recomputed `(delta_spy_s6, delta_qqq_s6, delta_iwm_s6)` matches
   the artifact's entry for that policy.

## Ready-made script

```
pip install solana solders pynacl requests
python research/tools/verify_aggregate_delta.py \\
    --rpc https://api.mainnet-beta.solana.com \\
    --aggregate-delta <AGG_PDA> \\
    --keeper-registry <KEEPER_REG_PDA>
```

Exit codes:

- `0` — signature valid, artifact Merkle root matches on-chain commitment
- `1` — signature verification failed (tampering or wrong keeper)
- `2` — IPFS fetch or Merkle round-trip failed (content unavailable or
        Merkle root mismatch)
- `3` — account parsing failed (likely schema drift — check the
        on-chain program version against `AggregateDelta::CURRENT_VERSION`)

Pass `--skip-ipfs` to run signature-only (useful when the local machine
is firewalled from public IPFS gateways; still catches keeper
tampering, does not catch off-chain-artifact drift).

## Failure semantics

- A signature mismatch in step 3 unambiguously means either the kernel
  did not verify the precompile correctly or the keeper was replaced
  without updating `KeeperRegistry.delta`. Both are incidents.
- A Merkle mismatch in step 5 means the artifact pinned to IPFS is not
  what the keeper signed. In practice this would require the keeper to
  have broadcast a transaction whose on-chain `merkle_root` disagreed
  with the content it pinned — which is exactly the provenance failure
  this field is designed to make externally visible.
- Per-feed monotonicity violations (Finding 2) cannot be observed from
  a single snapshot; they are caught on-chain at write time and would
  manifest as transaction failures with
  `KernelError::PythPublishTimeNotMonotonic` or
  `PythPublishTimeStale`.
