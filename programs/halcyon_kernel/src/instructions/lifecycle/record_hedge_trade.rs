use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

pub(crate) const HEDGE_RAW_SCALE: u128 = 1_000_000_000;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct RecordHedgeTradeArgs {
    pub product_program_id: Pubkey,
    pub asset_tag: [u8; 8],
    pub leg_index: u8,
    pub old_position_raw: i64,
    pub new_position_raw: i64,
    pub trade_delta_raw: i64,
    pub executed_price_s6: i64,
    pub execution_cost: u64,
    /// Monotonic keeper-assigned sequence. Must strictly exceed
    /// `hedge_book.sequence`. Prevents replay of a signed message and
    /// out-of-order reconciliation from a crashed keeper.
    pub sequence: u64,
}

#[derive(Accounts)]
#[instruction(args: RecordHedgeTradeArgs)]
pub struct RecordHedgeTrade<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    /// K13 — hedge book must belong to an actually-registered product. The
    /// kernel previously accepted any `product_program_id` off keeper input,
    /// which let a compromised keeper mint hedge books for non-existent products.
    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, args.product_program_id.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == args.product_program_id
            @ KernelError::HedgeBookProductMismatch,
        constraint = product_registry_entry.active @ HalcyonError::ProductNotRegistered,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + HedgeBookState::INIT_SPACE,
        seeds = [seeds::HEDGE_BOOK, args.product_program_id.as_ref()],
        bump,
    )]
    pub hedge_book: Account<'info, HedgeBookState>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + HedgeSleeve::INIT_SPACE,
        seeds = [seeds::HEDGE_SLEEVE, args.product_program_id.as_ref()],
        bump,
    )]
    pub hedge_sleeve: Account<'info, HedgeSleeve>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<RecordHedgeTrade>, args: RecordHedgeTradeArgs) -> Result<()> {
    let _ = ctx;
    let _ = args;
    err!(KernelError::HedgeTradeRecordingDisabled)
}

pub(crate) fn actual_trade_notional_usdc(
    trade_delta_raw: i64,
    executed_price_s6: i64,
) -> Result<u64> {
    require!(executed_price_s6 > 0, KernelError::InvalidExecutedPrice);

    let quantity_s6 = u128::from(trade_delta_raw.unsigned_abs());
    let price_s6 =
        u128::try_from(executed_price_s6).map_err(|_| error!(KernelError::InvalidExecutedPrice))?;
    let notional = quantity_s6
        .checked_mul(price_s6)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(HEDGE_RAW_SCALE)
        .ok_or(HalcyonError::Overflow)?;
    u64::try_from(notional).map_err(|_| error!(HalcyonError::Overflow))
}

pub(crate) fn apply_reserve_delta(current: u64, delta: i128) -> Result<u64> {
    let updated = i128::from(current)
        .checked_add(delta)
        .ok_or(HalcyonError::Overflow)?;
    require!(updated >= 0, HalcyonError::Overflow);
    u64::try_from(updated).map_err(|_| error!(HalcyonError::Overflow))
}

pub(crate) fn reference_notional_usdc_raw(
    position_delta_raw: u64,
    spot_price_s6: i64,
) -> Result<u64> {
    require!(spot_price_s6 > 0, KernelError::InvalidExecutedPrice);

    let raw = u128::from(position_delta_raw)
        .checked_mul(
            u128::try_from(spot_price_s6).map_err(|_| error!(KernelError::InvalidExecutedPrice))?,
        )
        .ok_or(HalcyonError::Overflow)?
        .checked_div(HEDGE_RAW_SCALE)
        .ok_or(HalcyonError::Overflow)?;
    u64::try_from(raw).map_err(|_| error!(HalcyonError::Overflow))
}
