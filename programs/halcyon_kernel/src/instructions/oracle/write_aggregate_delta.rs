use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions::load_instruction_at_checked;
use halcyon_common::{encode_aggregate_delta_message, seeds, HalcyonError};

use crate::{state::*, KernelError};

/// Non-negotiable Pyth publish_time lookahead: reject publish times more
/// than this many seconds in the future relative to block time. Catches
/// forged Pyth receiver accounts bearing future stamps.
const MAX_PYTH_PUBLISH_TIME_SKEW_SECS: i64 = 5;

/// Solana Ed25519 native precompile program ID. Hardcoded because
/// `anchor_lang::solana_program` does not re-export the constant on the
/// version pinned in this workspace. Value from
/// <https://docs.solana.com/developing/runtime-facilities/programs#ed25519-program>.
const ED25519_PROGRAM_ID: Pubkey = pubkey!("Ed25519SigVerify111111111111111111111111111");

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WriteAggregateDeltaArgs {
    pub product_program_id: Pubkey,
    pub delta_spy_s6: i64,
    pub delta_qqq_s6: i64,
    pub delta_iwm_s6: i64,
    pub merkle_root: [u8; 32],
    pub spot_spy_s6: i64,
    pub spot_qqq_s6: i64,
    pub spot_iwm_s6: i64,
    pub live_note_count: u32,
    /// [SPY, QQQ, IWM] Pyth publish_time seconds (audit F2).
    pub pyth_publish_times: [i64; 3],
    /// UTF-8 IPFS CID of the per-note artifact, zero-padded to 64 bytes
    /// (audit F4a). Empty CID is rejected on-chain.
    pub publication_cid: [u8; 64],
}

#[derive(Accounts)]
#[instruction(args: WriteAggregateDeltaArgs)]
pub struct WriteAggregateDelta<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, args.product_program_id.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == args.product_program_id
            @ KernelError::ProductProgramMismatch,
        constraint = product_registry_entry.active @ HalcyonError::ProductNotRegistered,
        // Audit F5 — a paused product should stop accruing fresh delta
        // snapshots; hedge decisions off a paused book confuse operators
        // and diverge from what any lifecycle write will accept.
        constraint = !product_registry_entry.paused @ HalcyonError::ProductPaused,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + AggregateDelta::INIT_SPACE,
        seeds = [seeds::AGGREGATE_DELTA, args.product_program_id.as_ref()],
        bump,
    )]
    pub aggregate_delta: Account<'info, AggregateDelta>,

    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: key is validated against the Solana instructions sysvar
    /// pubkey. Used only by `load_instruction_at_checked`.
    #[account(address = anchor_lang::solana_program::sysvar::instructions::ID)]
    pub instructions_sysvar: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WriteAggregateDelta>, args: WriteAggregateDeltaArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.delta,
        HalcyonError::KeeperAuthorityMismatch
    );

    // Publication CID must be non-empty — the off-chain artifact has to be
    // pinned before the kernel records its commitment.
    require!(
        args.publication_cid[0] != 0,
        KernelError::PublicationCidEmpty
    );

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let pyth_staleness_cap = ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs;

    // --- F2 — Pyth publish_time gates (staleness + clock-skew) ---
    for pt in args.pyth_publish_times.iter() {
        require!(*pt > 0, KernelError::PythPublishTimeStale);
        require!(
            *pt <= now.saturating_add(MAX_PYTH_PUBLISH_TIME_SKEW_SECS),
            KernelError::PythPublishTimeFuture
        );
        require!(
            now.saturating_sub(*pt) <= pyth_staleness_cap,
            KernelError::PythPublishTimeStale
        );
    }

    // --- F4b — Ed25519 precompile introspection ---
    // The keeper prepends an Ed25519 precompile instruction that Solana
    // validates natively. We load it at index 0 of the transaction and
    // confirm (a) it is the Ed25519 program, (b) the verified pubkey
    // matches `keeper_registry.delta`, (c) the verified message matches
    // the canonical encoding of these args + the incremented sequence.
    let sequence = ctx.accounts.aggregate_delta.sequence.saturating_add(1);
    let expected_msg = encode_aggregate_delta_message(
        &args.merkle_root,
        &args.pyth_publish_times,
        &[args.spot_spy_s6, args.spot_qqq_s6, args.spot_iwm_s6],
        sequence,
        &args.product_program_id,
    );
    let signature = verify_ed25519_precompile(
        &ctx.accounts.instructions_sysvar.to_account_info(),
        &ctx.accounts.keeper_registry.delta,
        &expected_msg,
    )?;

    // --- Commit ---
    let agg = &mut ctx.accounts.aggregate_delta;
    if agg.version == 0 {
        // Fresh account: initialise all per-feed publish times to the
        // incoming values (the monotonicity check would otherwise falsely
        // trigger against a zero-initialised slot).
        agg.version = AggregateDelta::CURRENT_VERSION;
        agg.product_program_id = args.product_program_id;
        agg.pyth_publish_times = args.pyth_publish_times;
    } else {
        // K10 — strict monotonicity on the trusted `now` timestamp.
        require!(
            now > agg.last_update_ts,
            HalcyonError::OracleTimestampNotMonotonic
        );
        // F2 — per-feed publish_time monotonicity. Defeats replay of a
        // stale-but-well-formed Pyth snapshot even within the staleness
        // window.
        for (new_pt, old_pt) in args
            .pyth_publish_times
            .iter()
            .zip(agg.pyth_publish_times.iter())
        {
            require!(*new_pt >= *old_pt, KernelError::PythPublishTimeNotMonotonic);
        }
        agg.pyth_publish_times = args.pyth_publish_times;
    }
    agg.delta_spy_s6 = args.delta_spy_s6;
    agg.delta_qqq_s6 = args.delta_qqq_s6;
    agg.delta_iwm_s6 = args.delta_iwm_s6;
    agg.merkle_root = args.merkle_root;
    agg.spot_spy_s6 = args.spot_spy_s6;
    agg.spot_qqq_s6 = args.spot_qqq_s6;
    agg.spot_iwm_s6 = args.spot_iwm_s6;
    agg.live_note_count = args.live_note_count;
    agg.last_update_ts = now;
    agg.last_update_slot = clock.slot;
    agg.sequence = sequence;
    agg.keeper_signature = signature;
    agg.publication_cid = args.publication_cid;
    Ok(())
}

/// Introspect the instructions sysvar for a preceding Ed25519 precompile
/// instruction and confirm it verified `(expected_pubkey, expected_msg)`.
/// Returns the 64-byte signature the precompile validated.
///
/// Solana's Ed25519 precompile instruction data layout (single-signature
/// case used here — `num_signatures = 1`):
///
/// ```text
///   byte  0         num_signatures (u8, must be 1)
///   byte  1         padding (u8, must be 0)
///   bytes 2..4      signature_offset         (u16 LE)
///   bytes 4..6      signature_instruction_index (u16 LE, 0xFFFF = this ix)
///   bytes 6..8      public_key_offset        (u16 LE)
///   bytes 8..10     public_key_instruction_index (u16 LE, 0xFFFF)
///   bytes 10..12    message_data_offset      (u16 LE)
///   bytes 12..14    message_data_size        (u16 LE)
///   bytes 14..16    message_instruction_index (u16 LE, 0xFFFF)
///   bytes 16..      pubkey ‖ signature ‖ message (the natural standard
///                   layout produced by solana-sdk ed25519_instruction::
///                   new_ed25519_instruction)
/// ```
///
/// The precompile's own signature verification runs ahead of this
/// instruction in the transaction pipeline; if it failed, the transaction
/// already aborted. What we do here is:
///   1. Confirm the precompile instruction exists (at index 0).
///   2. Parse it to extract pubkey, signature, and message.
///   3. Compare the extracted pubkey against the registered delta keeper.
///   4. Compare the extracted message against the canonical encoding of
///      the current args. Any mismatch aborts.
fn verify_ed25519_precompile(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &Pubkey,
    expected_msg: &[u8],
) -> Result<[u8; 64]> {
    let ix = load_instruction_at_checked(0, instructions_sysvar)
        .map_err(|_| error!(KernelError::MissingEd25519Instruction))?;

    require_keys_eq!(
        ix.program_id,
        ED25519_PROGRAM_ID,
        KernelError::MissingEd25519Instruction
    );

    parse_and_verify_ed25519_data(&ix.data, expected_pubkey, expected_msg)
}

/// Pure parser + verifier over an Ed25519 precompile instruction's `data`
/// field. Split out of `verify_ed25519_precompile` so it is reachable by
/// unit tests that cannot construct a real `AccountInfo` for the
/// instructions sysvar.
fn parse_and_verify_ed25519_data(
    data: &[u8],
    expected_pubkey: &Pubkey,
    expected_msg: &[u8],
) -> Result<[u8; 64]> {
    require!(data.len() >= 16, KernelError::MalformedEd25519Instruction);
    require!(data[0] == 1, KernelError::MalformedEd25519Instruction);
    require!(data[1] == 0, KernelError::MalformedEd25519Instruction);

    let read_u16 = |start: usize| -> u16 { u16::from_le_bytes([data[start], data[start + 1]]) };
    let sig_offset = read_u16(2) as usize;
    let sig_ix_idx = read_u16(4);
    let pk_offset = read_u16(6) as usize;
    let pk_ix_idx = read_u16(8);
    let msg_offset = read_u16(10) as usize;
    let msg_size = read_u16(12) as usize;
    let msg_ix_idx = read_u16(14);

    // All three offsets must reference the current precompile instruction's
    // own data. Allowing cross-instruction references would let a caller
    // point at data held in a different instruction and bypass our checks.
    require!(
        sig_ix_idx == u16::MAX && pk_ix_idx == u16::MAX && msg_ix_idx == u16::MAX,
        KernelError::MalformedEd25519Instruction
    );

    require!(
        sig_offset
            .checked_add(64)
            .ok_or(KernelError::MalformedEd25519Instruction)?
            <= data.len(),
        KernelError::MalformedEd25519Instruction
    );
    require!(
        pk_offset
            .checked_add(32)
            .ok_or(KernelError::MalformedEd25519Instruction)?
            <= data.len(),
        KernelError::MalformedEd25519Instruction
    );
    require!(
        msg_offset
            .checked_add(msg_size)
            .ok_or(KernelError::MalformedEd25519Instruction)?
            <= data.len(),
        KernelError::MalformedEd25519Instruction
    );

    let pubkey_bytes: [u8; 32] = data[pk_offset..pk_offset + 32]
        .try_into()
        .map_err(|_| error!(KernelError::MalformedEd25519Instruction))?;
    let verified_pubkey = Pubkey::new_from_array(pubkey_bytes);
    require_keys_eq!(
        verified_pubkey,
        *expected_pubkey,
        KernelError::Ed25519PubkeyMismatch
    );

    require!(
        msg_size == expected_msg.len(),
        KernelError::Ed25519MessageMismatch
    );
    require!(
        &data[msg_offset..msg_offset + msg_size] == expected_msg,
        KernelError::Ed25519MessageMismatch
    );

    let signature: [u8; 64] = data[sig_offset..sig_offset + 64]
        .try_into()
        .map_err(|_| error!(KernelError::MalformedEd25519Instruction))?;
    Ok(signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-build the precompile instruction data exactly the way the SDK
    /// helper does. Kept separate from the SDK helper so a drift between
    /// them is caught by both sides' tests.
    fn build_precompile_data(pubkey: &[u8; 32], signature: &[u8; 64], msg: &[u8]) -> Vec<u8> {
        const HEADER_LEN: u16 = 16;
        let pk_offset: u16 = HEADER_LEN;
        let sig_offset: u16 = pk_offset + 32;
        let msg_offset: u16 = sig_offset + 64;
        let msg_size: u16 = msg.len() as u16;
        let total_len = (msg_offset + msg_size) as usize;

        let mut data = vec![0u8; total_len];
        data[0] = 1;
        data[1] = 0;
        data[2..4].copy_from_slice(&sig_offset.to_le_bytes());
        data[4..6].copy_from_slice(&u16::MAX.to_le_bytes());
        data[6..8].copy_from_slice(&pk_offset.to_le_bytes());
        data[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        data[10..12].copy_from_slice(&msg_offset.to_le_bytes());
        data[12..14].copy_from_slice(&msg_size.to_le_bytes());
        data[14..16].copy_from_slice(&u16::MAX.to_le_bytes());
        data[pk_offset as usize..pk_offset as usize + 32].copy_from_slice(pubkey);
        data[sig_offset as usize..sig_offset as usize + 64].copy_from_slice(signature);
        data[msg_offset as usize..msg_offset as usize + msg_size as usize].copy_from_slice(msg);
        data
    }

    #[test]
    fn happy_path_extracts_signature() {
        let pubkey_bytes = [0xaa; 32];
        let signature = [0xbb; 64];
        let msg = b"hello world";
        let data = build_precompile_data(&pubkey_bytes, &signature, msg);
        let expected_pubkey = Pubkey::new_from_array(pubkey_bytes);
        let out = parse_and_verify_ed25519_data(&data, &expected_pubkey, msg).unwrap();
        assert_eq!(out, signature);
    }

    #[test]
    fn rejects_pubkey_mismatch() {
        let data = build_precompile_data(&[0xaa; 32], &[0xbb; 64], b"m");
        let wrong = Pubkey::new_from_array([0xcc; 32]);
        let err = parse_and_verify_ed25519_data(&data, &wrong, b"m").unwrap_err();
        assert!(
            format!("{err:?}").contains("Ed25519PubkeyMismatch"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_message_mismatch() {
        let data = build_precompile_data(&[0xaa; 32], &[0xbb; 64], b"one");
        let expected_pubkey = Pubkey::new_from_array([0xaa; 32]);
        let err = parse_and_verify_ed25519_data(&data, &expected_pubkey, b"two").unwrap_err();
        assert!(
            format!("{err:?}").contains("Ed25519MessageMismatch"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_cross_instruction_reference() {
        // Build normal data, then flip sig_ix_idx to 0 so it tries to read
        // from instruction index 0 instead of self-reference.
        let mut data = build_precompile_data(&[0xaa; 32], &[0xbb; 64], b"m");
        data[4..6].copy_from_slice(&0u16.to_le_bytes());
        let expected_pubkey = Pubkey::new_from_array([0xaa; 32]);
        let err = parse_and_verify_ed25519_data(&data, &expected_pubkey, b"m").unwrap_err();
        assert!(
            format!("{err:?}").contains("MalformedEd25519Instruction"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_wrong_num_signatures() {
        let mut data = build_precompile_data(&[0xaa; 32], &[0xbb; 64], b"m");
        data[0] = 2; // claim two signatures but only provide one
        let expected_pubkey = Pubkey::new_from_array([0xaa; 32]);
        let err = parse_and_verify_ed25519_data(&data, &expected_pubkey, b"m").unwrap_err();
        assert!(
            format!("{err:?}").contains("MalformedEd25519Instruction"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_truncated_data() {
        let err = parse_and_verify_ed25519_data(&[0u8; 8], &Pubkey::default(), b"m").unwrap_err();
        assert!(
            format!("{err:?}").contains("MalformedEd25519Instruction"),
            "unexpected err: {err:?}"
        );
    }
}
