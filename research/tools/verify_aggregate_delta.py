#!/usr/bin/env python3
"""External verifier for Halcyon flagship `AggregateDelta` provenance.

Reads an `AggregateDelta` account from-chain, extracts the keeper
signature, reconstructs the canonical signed bytes using the byte layout
documented in `crates/halcyon_common/src/aggregate_delta_signing.rs`, and
verifies the signature against the registered delta-keeper pubkey.
Optionally fetches the IPFS-pinned per-note artifact via the CID stored
on-chain, re-computes its Merkle root, and confirms it matches the
on-chain commitment.

This script is byte-explicit on purpose — an auditor can read it without
pulling the Rust workspace, and any drift between the on-chain encoding
and the keeper's signing input would produce a verifiable failure.

Dependencies (`pip install`):
  solana
  solders
  pynacl
  requests

Usage:
  verify_aggregate_delta.py --rpc https://api.devnet.solana.com \\
      --aggregate-delta <PUBKEY> --keeper-registry <PUBKEY> \\
      [--ipfs-gateway https://gateway.pinata.cloud]

Exit codes:
  0  all checks passed
  1  signature verification failed
  2  IPFS artifact fetch or Merkle round-trip failed
  3  account parsing failed (likely outdated schema)
"""
from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from dataclasses import dataclass
from typing import List, Tuple

import nacl.exceptions
import nacl.signing
import requests
from solana.rpc.api import Client
from solders.pubkey import Pubkey

# --- Canonical signed-message encoding ---
# Source: crates/halcyon_common/src/aggregate_delta_signing.rs
AGGREGATE_DELTA_DOMAIN_TAG = b"halcyon-aggregate-delta-v1\n"  # 27 bytes
AGGREGATE_DELTA_MESSAGE_LEN = 27 + 32 + 24 + 24 + 8 + 32  # 147 bytes


def encode_aggregate_delta_message(
    merkle_root: bytes,
    pyth_publish_times: Tuple[int, int, int],
    spot_snapshot_s6: Tuple[int, int, int],
    sequence: int,
    product_program_id: Pubkey,
) -> bytes:
    assert len(merkle_root) == 32
    buf = bytearray(AGGREGATE_DELTA_MESSAGE_LEN)
    buf[0:27] = AGGREGATE_DELTA_DOMAIN_TAG
    buf[27:59] = merkle_root
    for i, t in enumerate(pyth_publish_times):
        buf[59 + i * 8 : 67 + i * 8] = struct.pack("<q", t)
    for i, s in enumerate(spot_snapshot_s6):
        buf[83 + i * 8 : 91 + i * 8] = struct.pack("<q", s)
    buf[107:115] = struct.pack("<Q", sequence)
    buf[115:147] = bytes(product_program_id)
    return bytes(buf)


# --- AggregateDelta account layout (Anchor) ---
# Anchor account discriminator (8 bytes) + InitSpace-derived layout:
#   version: u8
#   product_program_id: Pubkey (32)
#   delta_spy_s6: i64
#   delta_qqq_s6: i64
#   delta_iwm_s6: i64
#   merkle_root: [u8; 32]
#   spot_spy_s6: i64
#   spot_qqq_s6: i64
#   spot_iwm_s6: i64
#   live_note_count: u32
#   last_update_slot: u64
#   last_update_ts: i64
#   pyth_publish_times: [i64; 3]
#   sequence: u64
#   keeper_signature: [u8; 64]
#   publication_cid: [u8; 64]


@dataclass
class AggregateDelta:
    version: int
    product_program_id: Pubkey
    delta_spy_s6: int
    delta_qqq_s6: int
    delta_iwm_s6: int
    merkle_root: bytes
    spot_spy_s6: int
    spot_qqq_s6: int
    spot_iwm_s6: int
    live_note_count: int
    last_update_slot: int
    last_update_ts: int
    pyth_publish_times: Tuple[int, int, int]
    sequence: int
    keeper_signature: bytes
    publication_cid: str


def parse_aggregate_delta(data: bytes) -> AggregateDelta:
    if len(data) < 8:
        raise ValueError("account data too short to contain Anchor discriminator")
    # Skip the 8-byte discriminator
    p = 8
    version = data[p]
    p += 1
    product_program_id = Pubkey.from_bytes(data[p : p + 32])
    p += 32
    delta_spy_s6 = struct.unpack("<q", data[p : p + 8])[0]
    p += 8
    delta_qqq_s6 = struct.unpack("<q", data[p : p + 8])[0]
    p += 8
    delta_iwm_s6 = struct.unpack("<q", data[p : p + 8])[0]
    p += 8
    merkle_root = bytes(data[p : p + 32])
    p += 32
    spot_spy_s6 = struct.unpack("<q", data[p : p + 8])[0]
    p += 8
    spot_qqq_s6 = struct.unpack("<q", data[p : p + 8])[0]
    p += 8
    spot_iwm_s6 = struct.unpack("<q", data[p : p + 8])[0]
    p += 8
    live_note_count = struct.unpack("<I", data[p : p + 4])[0]
    p += 4
    last_update_slot = struct.unpack("<Q", data[p : p + 8])[0]
    p += 8
    last_update_ts = struct.unpack("<q", data[p : p + 8])[0]
    p += 8
    pyth_publish_times = tuple(struct.unpack("<3q", data[p : p + 24]))
    p += 24
    sequence = struct.unpack("<Q", data[p : p + 8])[0]
    p += 8
    keeper_signature = bytes(data[p : p + 64])
    p += 64
    cid_bytes = bytes(data[p : p + 64])
    p += 64
    end = cid_bytes.find(b"\x00")
    cid_str = (cid_bytes if end < 0 else cid_bytes[:end]).decode("utf-8", errors="replace")
    return AggregateDelta(
        version=version,
        product_program_id=product_program_id,
        delta_spy_s6=delta_spy_s6,
        delta_qqq_s6=delta_qqq_s6,
        delta_iwm_s6=delta_iwm_s6,
        merkle_root=merkle_root,
        spot_spy_s6=spot_spy_s6,
        spot_qqq_s6=spot_qqq_s6,
        spot_iwm_s6=spot_iwm_s6,
        live_note_count=live_note_count,
        last_update_slot=last_update_slot,
        last_update_ts=last_update_ts,
        pyth_publish_times=pyth_publish_times,
        sequence=sequence,
        keeper_signature=keeper_signature,
        publication_cid=cid_str,
    )


def parse_keeper_registry_delta_pubkey(data: bytes) -> Pubkey:
    """Extract `KeeperRegistry.delta` from a fetched account.

    Layout (Anchor InitSpace):
      8 bytes discriminator
      u8     version
      Pubkey observation  (32)
      Pubkey regression   (32)
      Pubkey delta        (32)  <-- we want this one
      Pubkey hedge        (32)
      Pubkey regime       (32)
      i64    last_rotation_ts
    """
    if len(data) < 8 + 1 + 32 * 5:
        raise ValueError("keeper_registry account too short")
    offset = 8 + 1 + 32 * 2
    return Pubkey.from_bytes(data[offset : offset + 32])


def merkle_leaf_hash(policy_pubkey: Pubkey, delta_spy_s6: int, delta_qqq_s6: int, delta_iwm_s6: int) -> bytes:
    """Matches `leaf_hash` in keepers/delta_keeper/src/main.rs."""
    h = hashlib.sha256()
    h.update(b"flagship-delta-leaf")
    h.update(bytes(policy_pubkey))
    h.update(struct.pack("<q", delta_spy_s6))
    h.update(struct.pack("<q", delta_qqq_s6))
    h.update(struct.pack("<q", delta_iwm_s6))
    return h.digest()


def merkle_node_hash(left: bytes, right: bytes) -> bytes:
    h = hashlib.sha256()
    h.update(b"flagship-delta-node")
    h.update(left)
    h.update(right)
    return h.digest()


def compute_merkle_root(leaves: List[bytes]) -> bytes:
    if not leaves:
        return b"\x00" * 32
    level = list(leaves)
    while len(level) > 1:
        nxt = []
        for i in range(0, len(level), 2):
            left = level[i]
            right = level[i + 1] if i + 1 < len(level) else left
            nxt.append(merkle_node_hash(left, right))
        level = nxt
    return level[0]


def fetch_ipfs_artifact(cid: str, gateway: str) -> dict:
    url = f"{gateway.rstrip('/')}/ipfs/{cid}"
    response = requests.get(url, timeout=30)
    response.raise_for_status()
    return response.json()


def verify_merkle_from_artifact(artifact: dict, expected_root: bytes) -> bool:
    leaves = []
    for note in artifact["artifact"]["notes"]:
        policy = Pubkey.from_string(note["policy"])
        leaves.append(
            merkle_leaf_hash(
                policy,
                int(note["delta_spy_s6"]),
                int(note["delta_qqq_s6"]),
                int(note["delta_iwm_s6"]),
            )
        )
    computed = compute_merkle_root(leaves)
    return computed == expected_root


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--rpc", required=True, help="Solana RPC endpoint")
    parser.add_argument("--aggregate-delta", required=True, help="AggregateDelta PDA pubkey")
    parser.add_argument("--keeper-registry", required=True, help="KeeperRegistry PDA pubkey")
    parser.add_argument(
        "--ipfs-gateway",
        default="https://gateway.pinata.cloud",
        help="HTTPS IPFS gateway to use for artifact retrieval",
    )
    parser.add_argument(
        "--skip-ipfs",
        action="store_true",
        help="Skip IPFS artifact fetch and Merkle round-trip (signature-only)",
    )
    args = parser.parse_args()

    rpc = Client(args.rpc)

    agg_info = rpc.get_account_info(Pubkey.from_string(args.aggregate_delta))
    if agg_info.value is None:
        print(f"error: AggregateDelta account {args.aggregate_delta} not found", file=sys.stderr)
        return 3
    try:
        agg = parse_aggregate_delta(bytes(agg_info.value.data))
    except Exception as err:
        print(f"error: failed to parse AggregateDelta: {err}", file=sys.stderr)
        return 3

    reg_info = rpc.get_account_info(Pubkey.from_string(args.keeper_registry))
    if reg_info.value is None:
        print(f"error: KeeperRegistry account {args.keeper_registry} not found", file=sys.stderr)
        return 3
    keeper_pubkey = parse_keeper_registry_delta_pubkey(bytes(reg_info.value.data))

    print(f"AggregateDelta account: {args.aggregate_delta}")
    print(f"  version           : {agg.version}")
    print(f"  product_program_id: {agg.product_program_id}")
    print(f"  sequence          : {agg.sequence}")
    print(f"  pyth_publish_times: {agg.pyth_publish_times}")
    print(f"  merkle_root       : {agg.merkle_root.hex()}")
    print(f"  publication_cid   : {agg.publication_cid or '(empty)'}")
    print(f"Registered delta keeper: {keeper_pubkey}")

    # --- Signature verification ---
    canonical = encode_aggregate_delta_message(
        agg.merkle_root,
        agg.pyth_publish_times,
        (agg.spot_spy_s6, agg.spot_qqq_s6, agg.spot_iwm_s6),
        agg.sequence,
        agg.product_program_id,
    )
    assert len(canonical) == AGGREGATE_DELTA_MESSAGE_LEN
    verify_key = nacl.signing.VerifyKey(bytes(keeper_pubkey))
    try:
        verify_key.verify(canonical, agg.keeper_signature)
    except nacl.exceptions.BadSignatureError:
        print("FAIL: ed25519 signature does not verify against the registered delta keeper", file=sys.stderr)
        return 1
    print("OK  : keeper_signature verifies against KeeperRegistry.delta")

    if args.skip_ipfs:
        print("SKIP: --skip-ipfs set; not fetching IPFS artifact")
        return 0

    if not agg.publication_cid:
        print("FAIL: publication_cid is empty; no off-chain artifact to verify", file=sys.stderr)
        return 2

    try:
        artifact = fetch_ipfs_artifact(agg.publication_cid, args.ipfs_gateway)
    except Exception as err:
        print(f"FAIL: IPFS fetch via {args.ipfs_gateway} failed: {err}", file=sys.stderr)
        return 2

    if not verify_merkle_from_artifact(artifact, agg.merkle_root):
        print("FAIL: Merkle root of IPFS artifact does not match on-chain merkle_root", file=sys.stderr)
        return 2
    print(f"OK  : Merkle root of IPFS artifact matches on-chain commitment ({len(artifact['artifact']['notes'])} notes)")

    return 0


if __name__ == "__main__":
    sys.exit(main())
