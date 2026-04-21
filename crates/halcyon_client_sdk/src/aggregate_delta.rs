//! Helpers for signing canonical `AggregateDelta` messages (audit F4b).
//!
//! The delta keeper submits `write_aggregate_delta` as a two-instruction
//! transaction:
//!
//!   Ix[0]: Ed25519 precompile, verifies the keeper's signature over
//!          `encode_aggregate_delta_message(...)`.
//!   Ix[1]: `write_aggregate_delta` itself, which introspects the sysvar
//!          instructions list to confirm Ix[0] is a correctly-structured
//!          Ed25519 verification of the same canonical message.
//!
//! [`build_signed_write_aggregate_delta_ixs`] produces both instructions
//! from the keeper's keypair, the args, and the *expected next* sequence
//! number (the keeper reads the on-chain account, adds 1, and passes that
//! value).

use halcyon_common::{encode_aggregate_delta_message, AGGREGATE_DELTA_MESSAGE_LEN};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signature,
    signer::{keypair::Keypair, Signer},
};

use crate::kernel::{write_aggregate_delta_ix, WriteAggregateDeltaArgs};

/// Solana Ed25519 native precompile program ID. Hardcoded to sidestep
/// differences in re-export paths across solana-sdk versions. Same
/// constant the kernel uses in `write_aggregate_delta`.
const ED25519_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("Ed25519SigVerify111111111111111111111111111");

/// Build the Ed25519 precompile instruction used to front
/// `write_aggregate_delta`. The kernel introspects this instruction's data
/// layout, so the byte-exact structure here must match the parser in
/// `programs/halcyon_kernel/src/instructions/oracle/write_aggregate_delta.rs`.
pub fn build_aggregate_delta_ed25519_ix(
    keeper_pubkey: &Pubkey,
    signature: &Signature,
    canonical_msg: &[u8; AGGREGATE_DELTA_MESSAGE_LEN],
) -> Instruction {
    // Solana's Ed25519 precompile single-signature layout:
    //   [num_sigs=1, padding=0]
    //   [sig_offset u16][sig_ix_idx u16=0xFFFF]
    //   [pk_offset  u16][pk_ix_idx  u16=0xFFFF]
    //   [msg_offset u16][msg_size   u16][msg_ix_idx u16=0xFFFF]
    //   [pubkey 32 bytes]
    //   [signature 64 bytes]
    //   [message N bytes]
    const HEADER_LEN: u16 = 16;
    let pk_offset: u16 = HEADER_LEN;
    let sig_offset: u16 = pk_offset + 32;
    let msg_offset: u16 = sig_offset + 64;
    let msg_size: u16 = AGGREGATE_DELTA_MESSAGE_LEN as u16;
    let total_len = (msg_offset + msg_size) as usize;

    let mut data = vec![0u8; total_len];
    data[0] = 1; // num_signatures
    data[1] = 0; // padding
    data[2..4].copy_from_slice(&sig_offset.to_le_bytes());
    data[4..6].copy_from_slice(&u16::MAX.to_le_bytes());
    data[6..8].copy_from_slice(&pk_offset.to_le_bytes());
    data[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
    data[10..12].copy_from_slice(&msg_offset.to_le_bytes());
    data[12..14].copy_from_slice(&msg_size.to_le_bytes());
    data[14..16].copy_from_slice(&u16::MAX.to_le_bytes());

    data[pk_offset as usize..pk_offset as usize + 32].copy_from_slice(&keeper_pubkey.to_bytes());
    data[sig_offset as usize..sig_offset as usize + 64].copy_from_slice(&signature.as_ref());
    data[msg_offset as usize..msg_offset as usize + msg_size as usize]
        .copy_from_slice(canonical_msg);

    Instruction {
        program_id: ED25519_PROGRAM_ID,
        accounts: Vec::<AccountMeta>::new(),
        data,
    }
}

/// Compose the `[ed25519_ix, write_aggregate_delta_ix]` pair. `args` must
/// carry the same `merkle_root`, `pyth_publish_times`, spots, and
/// `product_program_id` that are used to produce the canonical bytes;
/// `next_sequence` is the value that will be stamped on-chain after a
/// successful write (i.e. on-chain stored `sequence + 1` for an existing
/// account, or `1` for a fresh one).
pub fn build_signed_write_aggregate_delta_ixs(
    keeper: &Keypair,
    payer: &Pubkey,
    args: &WriteAggregateDeltaArgs,
    next_sequence: u64,
) -> (Instruction, Instruction) {
    let canonical = encode_aggregate_delta_message(
        &args.merkle_root,
        &args.pyth_publish_times,
        &[args.spot_spy_s6, args.spot_qqq_s6, args.spot_iwm_s6],
        next_sequence,
        &args.product_program_id,
    );
    let signature = keeper.sign_message(&canonical);
    let ed_ix = build_aggregate_delta_ed25519_ix(&keeper.pubkey(), &signature, &canonical);
    let write_ix = write_aggregate_delta_ix(&keeper.pubkey(), payer, args.clone());
    (ed_ix, write_ix)
}

/// Pack a UTF-8 IPFS CID into the fixed-width zero-padded byte array the
/// on-chain struct holds. Fails if the CID exceeds 64 bytes.
pub fn encode_publication_cid(cid: &str) -> anyhow::Result<[u8; 64]> {
    anyhow::ensure!(!cid.is_empty(), "publication CID must not be empty");
    anyhow::ensure!(
        cid.len() <= 64,
        "publication CID must fit in 64 bytes (got {})",
        cid.len()
    );
    let mut out = [0u8; 64];
    out[..cid.len()].copy_from_slice(cid.as_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_encoding_round_trip() {
        let cid = "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi";
        let bytes = encode_publication_cid(cid).unwrap();
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        assert_eq!(std::str::from_utf8(&bytes[..end]).unwrap(), cid);
    }

    #[test]
    fn cid_encoding_rejects_oversize() {
        let long_cid = "x".repeat(65);
        assert!(encode_publication_cid(&long_cid).is_err());
    }

    #[test]
    fn cid_encoding_rejects_empty() {
        assert!(encode_publication_cid("").is_err());
    }
}
