use anchor_lang::prelude::*;

/// Flagship-only: 3D aggregate delta over SPY/QQQ/IWM, plus the on-chain
/// commitment trail that lets an external auditor verify who wrote each
/// snapshot and what off-chain per-note breakdown it attests to.
///
/// Version 2 (Layer-5 audit remediation):
///   - `pyth_publish_times` — per-feed Pyth publish_time (Finding 2). Enforced
///     monotonic per-feed by `write_aggregate_delta`; any of the three feeds
///     going backward aborts the write.
///   - `keeper_signature` + `sequence` — Ed25519 signature over the canonical
///     `encode_aggregate_delta_message` bytes (Finding 4b). The kernel verifies
///     the signature via the native Ed25519 precompile before stamping it
///     here; a reader of this account can replay the verification offline
///     without fetching the original transaction.
///   - `publication_cid` — IPFS CID of the per-note artifact committed by
///     `merkle_root` (Finding 4a). Fixed-width UTF-8 bytes padded with
///     trailing zeros; the first zero byte terminates. CIDv0 Qm-prefixed
///     strings are 46 bytes, CIDv1 base32 bafy-prefixed strings are 59.
///     64 bytes leaves headroom for future encodings without another
///     migration.
#[account]
#[derive(InitSpace)]
pub struct AggregateDelta {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub delta_spy_s6: i64,
    pub delta_qqq_s6: i64,
    pub delta_iwm_s6: i64,
    pub merkle_root: [u8; 32],
    pub spot_spy_s6: i64,
    pub spot_qqq_s6: i64,
    pub spot_iwm_s6: i64,
    pub live_note_count: u32,
    pub last_update_slot: u64,
    pub last_update_ts: i64,
    /// [SPY, QQQ, IWM] Pyth publish_time seconds (Finding 2).
    pub pyth_publish_times: [i64; 3],
    /// Monotonic counter incremented on every successful write. Part of the
    /// signed canonical message (Finding 4b); prevents replay of an earlier
    /// valid (sig, args) pair.
    pub sequence: u64,
    /// Ed25519 signature by the registered delta keeper over
    /// `encode_aggregate_delta_message(...)` (Finding 4b). Verified on-chain
    /// via the native Ed25519 precompile + instructions-sysvar introspection.
    pub keeper_signature: [u8; 64],
    /// UTF-8-encoded IPFS CID of the canonical per-note artifact whose root
    /// equals `merkle_root` (Finding 4a). Trailing zero bytes are padding;
    /// readers should trim at the first 0x00.
    pub publication_cid: [u8; 64],
}

impl AggregateDelta {
    /// v1 → v2: added `pyth_publish_times`, `sequence`, `keeper_signature`,
    /// `publication_cid`. Pre-mainnet, so live devnet accounts are expected
    /// to be recreated on redeploy; the `version == 0` branch in the
    /// handler is the "fresh account" path.
    pub const CURRENT_VERSION: u8 = 2;

    /// Trim the trailing zero-byte padding from `publication_cid`.
    pub fn publication_cid_as_str(&self) -> &str {
        let end = self
            .publication_cid
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(self.publication_cid.len());
        core::str::from_utf8(&self.publication_cid[..end]).unwrap_or("")
    }
}
