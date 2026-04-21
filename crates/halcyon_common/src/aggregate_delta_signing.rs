//! Canonical message encoding for flagship `AggregateDelta` keeper signatures
//! (audit Finding 4b). The delta keeper signs this byte-string with its
//! Ed25519 authority keypair; `write_aggregate_delta` verifies the same
//! bytes on-chain via the Ed25519 precompile + instructions-sysvar
//! introspection.
//!
//! The encoding is hand-rolled fixed-width little-endian rather than Borsh
//! so external auditors can reconstruct the signed bytes without pulling
//! the workspace in. Exact layout:
//!
//! ```text
//!   [ 0.. 27]  domain-separation tag  = AGGREGATE_DELTA_DOMAIN_TAG (27 bytes)
//!   [27.. 59]  merkle_root             (32 bytes)
//!   [59.. 83]  pyth_publish_times[0..3] (3 × i64 LE, order: SPY, QQQ, IWM)
//!   [83..107]  spot_snapshot[0..3]      (3 × i64 LE, SCALE_6; SPY, QQQ, IWM)
//!   [107..115] sequence                 (u64 LE)
//!   [115..147] product_program_id       (32 bytes)
//! ```
//!
//! Total length = `AGGREGATE_DELTA_MESSAGE_LEN` (147 bytes). Sequence is the
//! monotonically-increasing counter stored on `AggregateDelta`; it rolls
//! forward on every successful write and defeats replay of a prior valid
//! signature even when all other fields happen to match.

use anchor_lang::prelude::Pubkey;

/// Domain-separation tag. Exactly 27 bytes. Bumping the `v1` suffix is the
/// way to make old signatures un-verifiable by a newer kernel.
pub const AGGREGATE_DELTA_DOMAIN_TAG: &[u8; 27] = b"halcyon-aggregate-delta-v1\n";

/// Total length of the canonical signed message.
pub const AGGREGATE_DELTA_MESSAGE_LEN: usize = 27 + 32 + 24 + 24 + 8 + 32;

/// Produce the canonical bytes the keeper signs and the kernel verifies.
pub fn encode_aggregate_delta_message(
    merkle_root: &[u8; 32],
    pyth_publish_times: &[i64; 3],
    spot_snapshot_s6: &[i64; 3],
    sequence: u64,
    product_program_id: &Pubkey,
) -> [u8; AGGREGATE_DELTA_MESSAGE_LEN] {
    let mut out = [0u8; AGGREGATE_DELTA_MESSAGE_LEN];
    out[0..27].copy_from_slice(AGGREGATE_DELTA_DOMAIN_TAG);
    out[27..59].copy_from_slice(merkle_root);
    for (i, t) in pyth_publish_times.iter().enumerate() {
        out[59 + i * 8..59 + (i + 1) * 8].copy_from_slice(&t.to_le_bytes());
    }
    for (i, s) in spot_snapshot_s6.iter().enumerate() {
        out[83 + i * 8..83 + (i + 1) * 8].copy_from_slice(&s.to_le_bytes());
    }
    out[107..115].copy_from_slice(&sequence.to_le_bytes());
    out[115..147].copy_from_slice(product_program_id.as_ref());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_is_fixed_width_and_deterministic() {
        let root = [0xab; 32];
        let publish = [1_700_000_000i64, 1_700_000_001, 1_700_000_002];
        let spots = [420_123_456i64, 350_987_654, 195_432_100];
        let product = Pubkey::new_from_array([0x11; 32]);
        let a = encode_aggregate_delta_message(&root, &publish, &spots, 7, &product);
        let b = encode_aggregate_delta_message(&root, &publish, &spots, 7, &product);
        assert_eq!(a, b);
        assert_eq!(a.len(), AGGREGATE_DELTA_MESSAGE_LEN);
    }

    #[test]
    fn layout_matches_documentation() {
        let root = [0u8; 32];
        let publish = [1i64, 2, 3];
        let spots = [4i64, 5, 6];
        let product = Pubkey::new_from_array([0u8; 32]);
        let msg = encode_aggregate_delta_message(&root, &publish, &spots, 42, &product);
        assert_eq!(&msg[0..27], AGGREGATE_DELTA_DOMAIN_TAG);
        assert_eq!(i64::from_le_bytes(msg[59..67].try_into().unwrap()), 1);
        assert_eq!(i64::from_le_bytes(msg[67..75].try_into().unwrap()), 2);
        assert_eq!(i64::from_le_bytes(msg[75..83].try_into().unwrap()), 3);
        assert_eq!(i64::from_le_bytes(msg[83..91].try_into().unwrap()), 4);
        assert_eq!(i64::from_le_bytes(msg[91..99].try_into().unwrap()), 5);
        assert_eq!(i64::from_le_bytes(msg[99..107].try_into().unwrap()), 6);
        assert_eq!(u64::from_le_bytes(msg[107..115].try_into().unwrap()), 42);
    }

    #[test]
    fn sequence_change_produces_different_message() {
        let root = [0xab; 32];
        let publish = [1i64, 2, 3];
        let spots = [4i64, 5, 6];
        let product = Pubkey::new_from_array([0x11; 32]);
        let a = encode_aggregate_delta_message(&root, &publish, &spots, 1, &product);
        let b = encode_aggregate_delta_message(&root, &publish, &spots, 2, &product);
        assert_ne!(a, b);
    }
}
