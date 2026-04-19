use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Revoke, Token, TokenAccount};
use halcyon_common::{events::HedgeBookUpdated, seeds, HalcyonError};

use crate::{state::*, KernelError};

pub(crate) const HEDGE_RAW_SCALE: u128 = 1_000_000_000;
pub(crate) const JUPITER_V6_PROGRAM_ID: Pubkey =
    pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct RecordHedgeTradeArgs {
    pub product_program_id: Pubkey,
    pub sequence: u64,
}

#[derive(Accounts)]
#[instruction(args: RecordHedgeTradeArgs)]
pub struct RecordHedgeTrade<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Box<Account<'info, KeeperRegistry>>,

    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, args.product_program_id.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == args.product_program_id
            @ KernelError::HedgeBookProductMismatch,
        constraint = product_registry_entry.active @ HalcyonError::ProductNotRegistered,
        constraint = !product_registry_entry.paused @ HalcyonError::ProductPaused,
    )]
    pub product_registry_entry: Box<Account<'info, ProductRegistryEntry>>,

    #[account(
        mut,
        seeds = [seeds::HEDGE_BOOK, args.product_program_id.as_ref()],
        bump,
        constraint = hedge_book.product_program_id == args.product_program_id
            @ KernelError::HedgeBookProductMismatch,
    )]
    pub hedge_book: Box<Account<'info, HedgeBookState>>,

    #[account(
        mut,
        seeds = [seeds::HEDGE_SLEEVE, args.product_program_id.as_ref()],
        bump,
        constraint = hedge_sleeve.product_program_id == args.product_program_id
            @ KernelError::HedgeBookProductMismatch,
    )]
    pub hedge_sleeve: Box<Account<'info, HedgeSleeve>>,

    #[account(
        mut,
        seeds = [seeds::PENDING_HEDGE_SWAP, args.product_program_id.as_ref()],
        bump,
        constraint = pending_hedge_swap.product_program_id == args.product_program_id
            @ KernelError::HedgeBookProductMismatch,
    )]
    pub pending_hedge_swap: Box<Account<'info, PendingHedgeSwap>>,

    #[account(
        mut,
        constraint = hedge_sleeve_usdc.key()
            == anchor_spl::associated_token::get_associated_token_address(
                &hedge_sleeve.key(),
                &usdc_mint.key(),
            ) @ KernelError::HedgeBookProductMismatch,
        constraint = usdc_mint.key() == hedge_sleeve_usdc.mint
            @ KernelError::HedgeBookProductMismatch,
        constraint = hedge_sleeve_usdc.owner == hedge_sleeve.key()
            @ KernelError::HedgeBookProductMismatch,
    )]
    pub hedge_sleeve_usdc: Box<Account<'info, TokenAccount>>,

    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = hedge_sleeve_wsol.key()
            == anchor_spl::associated_token::get_associated_token_address(
                &hedge_sleeve.key(),
                &anchor_spl::token::spl_token::native_mint::ID,
            ) @ KernelError::HedgeBookProductMismatch,
        constraint = hedge_sleeve_wsol.owner == hedge_sleeve.key()
            @ KernelError::HedgeBookProductMismatch,
        constraint = hedge_sleeve_wsol.mint == anchor_spl::token::spl_token::native_mint::ID
            @ KernelError::HedgeBookProductMismatch,
    )]
    pub hedge_sleeve_wsol: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<RecordHedgeTrade>, args: RecordHedgeTradeArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.hedge,
        HalcyonError::KeeperAuthorityMismatch
    );

    let pending = &ctx.accounts.pending_hedge_swap;
    require!(pending.active, KernelError::PendingHedgeSwapMissing);
    require_keys_eq!(
        pending.product_program_id,
        args.product_program_id,
        KernelError::PendingHedgeSwapMismatch
    );
    require_keys_eq!(
        pending.keeper,
        ctx.accounts.keeper.key(),
        KernelError::PendingHedgeSwapMismatch
    );
    require!(
        pending.sequence == args.sequence,
        KernelError::PendingHedgeSwapMismatch
    );
    require!(
        args.sequence > ctx.accounts.hedge_book.sequence,
        HalcyonError::HedgeSequenceNotMonotonic
    );
    let leg_index = pending.leg_index;
    let target_position_raw = pending.target_position_raw;
    let asset_tag = pending.asset_tag;
    let source_is_wsol = pending.source_is_wsol;
    let old_position_raw = pending.old_position_raw;
    let min_position_raw = pending.min_position_raw;
    let max_position_raw = pending.max_position_raw;
    let approved_input_amount = pending.approved_input_amount;
    let source_balance_before = pending.source_balance_before;
    let destination_balance_before = pending.destination_balance_before;
    let spot_price_s6 = pending.spot_price_s6;
    let max_slippage_bps = pending.max_slippage_bps;
    let now = Clock::get()?.unix_timestamp;

    let post_usdc_balance = ctx.accounts.hedge_sleeve_usdc.amount;
    let post_wsol_balance = ctx.accounts.hedge_sleeve_wsol.amount;
    let source_balance_after = if source_is_wsol {
        post_wsol_balance
    } else {
        post_usdc_balance
    };
    let destination_balance_after = if source_is_wsol {
        post_usdc_balance
    } else {
        post_wsol_balance
    };
    require!(
        source_balance_before >= source_balance_after,
        KernelError::InvalidHedgeSwapBalanceDelta
    );
    require!(
        destination_balance_after >= destination_balance_before,
        KernelError::InvalidHedgeSwapBalanceDelta
    );

    let source_spent_raw = source_balance_before
        .checked_sub(source_balance_after)
        .ok_or(HalcyonError::Overflow)?;
    let destination_gained_raw = destination_balance_after
        .checked_sub(destination_balance_before)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        source_spent_raw > 0 && destination_gained_raw > 0,
        KernelError::InvalidHedgeSwapBalanceDelta
    );
    require!(
        source_spent_raw <= approved_input_amount,
        KernelError::InvalidHedgeSwapBalanceDelta
    );

    let new_position_raw =
        i64::try_from(post_wsol_balance).map_err(|_| error!(HalcyonError::Overflow))?;
    let trade_delta_raw = new_position_raw
        .checked_sub(old_position_raw)
        .ok_or(HalcyonError::Overflow)?;
    let pre_usdc_balance = if source_is_wsol {
        destination_balance_before
    } else {
        source_balance_before
    };
    let actual_usdc_delta_raw = i64::try_from(post_usdc_balance)
        .map_err(|_| error!(HalcyonError::Overflow))?
        .checked_sub(i64::try_from(pre_usdc_balance).map_err(|_| error!(HalcyonError::Overflow))?)
        .ok_or(HalcyonError::Overflow)?;

    if source_is_wsol {
        require!(
            trade_delta_raw < 0 && actual_usdc_delta_raw > 0,
            KernelError::InvalidHedgeSwapBalanceDelta
        );
    } else {
        require!(
            trade_delta_raw > 0 && actual_usdc_delta_raw < 0,
            KernelError::InvalidHedgeSwapBalanceDelta
        );
    }

    let position_delta_abs_raw = trade_delta_raw.unsigned_abs();
    let usdc_flow_abs_raw = actual_usdc_delta_raw.unsigned_abs();
    require!(
        position_delta_abs_raw > 0 && usdc_flow_abs_raw > 0,
        KernelError::InvalidHedgeSwapBalanceDelta
    );
    require!(
        new_position_raw >= min_position_raw && new_position_raw <= max_position_raw,
        KernelError::ExecutedHedgeOutsideBounds
    );

    let executed_price_s6 = effective_price_s6(usdc_flow_abs_raw, position_delta_abs_raw)?;
    let executed_slippage_bps = price_deviation_bps(spot_price_s6, executed_price_s6)?;
    require!(
        executed_slippage_bps <= u64::from(max_slippage_bps),
        HalcyonError::SlippageExceeded
    );

    let reference_notional_raw =
        reference_notional_usdc_raw(position_delta_abs_raw, spot_price_s6)?;
    let execution_cost = usdc_flow_abs_raw.saturating_sub(reference_notional_raw);

    let revoke_source = if source_is_wsol {
        &ctx.accounts.hedge_sleeve_wsol
    } else {
        &ctx.accounts.hedge_sleeve_usdc
    };
    if revoke_source.delegate.is_some() {
        let bump = ctx.bumps.hedge_sleeve;
        let signer_seeds: &[&[&[u8]]] = &[&[
            seeds::HEDGE_SLEEVE,
            args.product_program_id.as_ref(),
            &[bump],
        ]];
        token::revoke(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Revoke {
                source: revoke_source.to_account_info(),
                authority: ctx.accounts.hedge_sleeve.to_account_info(),
            },
            signer_seeds,
        ))?;
    }

    let book = &mut ctx.accounts.hedge_book;
    let leg_idx = leg_index as usize;
    if leg_idx >= book.leg_count as usize {
        book.leg_count = leg_index.saturating_add(1);
    }
    let leg = &mut book.legs[leg_idx];
    leg.asset_tag = asset_tag;
    leg.current_position_raw = new_position_raw;
    leg.target_position_raw = target_position_raw;
    leg.last_rebalance_ts = now;
    leg.last_rebalance_price_s6 = executed_price_s6;

    let sleeve = &mut ctx.accounts.hedge_sleeve;
    sleeve.usdc_reserve = post_usdc_balance;
    sleeve.lifetime_execution_cost = sleeve
        .lifetime_execution_cost
        .checked_add(execution_cost)
        .ok_or(HalcyonError::Overflow)?;
    sleeve.last_update_ts = now;

    book.cumulative_execution_cost = book
        .cumulative_execution_cost
        .checked_add(execution_cost)
        .ok_or(HalcyonError::Overflow)?;
    book.last_rebalance_ts = now;
    book.sequence = args.sequence;

    let pending = &mut ctx.accounts.pending_hedge_swap;
    pending.active = false;
    pending.keeper = Pubkey::default();
    pending.sequence = 0;
    pending.approved_input_amount = 0;

    emit!(HedgeBookUpdated {
        product_program_id: args.product_program_id,
        hedge_book: book.key(),
        leg_index,
        new_position_raw,
        trade_delta_raw,
        executed_price_s6,
        updated_at: now,
    });
    Ok(())
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

pub(crate) fn effective_price_s6(usdc_raw: u64, sol_raw: u64) -> Result<i64> {
    require!(
        usdc_raw > 0 && sol_raw > 0,
        KernelError::InvalidExecutedPrice
    );
    let raw = u128::from(usdc_raw)
        .checked_mul(HEDGE_RAW_SCALE)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(u128::from(sol_raw))
        .ok_or(HalcyonError::Overflow)?;
    i64::try_from(raw).map_err(|_| error!(HalcyonError::Overflow))
}

pub(crate) fn price_deviation_bps(reference_price_s6: i64, observed_price_s6: i64) -> Result<u64> {
    require!(
        reference_price_s6 > 0 && observed_price_s6 > 0,
        KernelError::InvalidOraclePrice
    );
    let diff = (i128::from(reference_price_s6) - i128::from(observed_price_s6)).abs() as u128;
    let bps = diff
        .checked_mul(10_000u128)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(
            u128::try_from(reference_price_s6).map_err(|_| error!(HalcyonError::Overflow))?,
        )
        .ok_or(HalcyonError::Overflow)?;
    u64::try_from(bps).map_err(|_| error!(HalcyonError::Overflow))
}
