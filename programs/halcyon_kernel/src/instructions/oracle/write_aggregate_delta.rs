use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

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
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

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

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WriteAggregateDelta>, args: WriteAggregateDeltaArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.delta,
        HalcyonError::KeeperAuthorityMismatch
    );

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let agg = &mut ctx.accounts.aggregate_delta;
    if agg.version == 0 {
        agg.version = AggregateDelta::CURRENT_VERSION;
        agg.product_program_id = args.product_program_id;
    } else {
        // K10 — strict monotonicity on the trusted `now` timestamp.
        // No rate-limit gate on the delta keeper — it runs at 15-30s during
        // market hours (per L4 plan §3.6) and rate-limiting would defeat the
        // design. Monotonicity alone catches replay / reordering.
        require!(
            now > agg.last_update_ts,
            HalcyonError::OracleTimestampNotMonotonic
        );
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
    Ok(())
}
