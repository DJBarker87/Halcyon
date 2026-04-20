use anchor_lang::{
    prelude::*,
    solana_program::sysvar::instructions::{
        load_current_index_checked, load_instruction_at_checked,
    },
    Discriminator,
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Approve, Mint, Token, TokenAccount},
};
use halcyon_common::{seeds, HalcyonError};

use crate::{
    instructions::lifecycle::record_hedge_trade::{
        reference_notional_usdc_raw, RecordHedgeTradeArgs, HEDGE_RAW_SCALE, JUPITER_V6_PROGRAM_ID,
    },
    state::*,
    KernelError,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct PrepareHedgeSwapArgs {
    pub product_program_id: Pubkey,
    pub asset_tag: [u8; 8],
    pub leg_index: u8,
    pub old_position_raw: i64,
    pub target_position_raw: i64,
    pub min_position_raw: i64,
    pub max_position_raw: i64,
    pub approved_input_amount: u64,
    pub max_slippage_bps: u16,
    pub sequence: u64,
}

#[derive(Accounts)]
#[instruction(args: PrepareHedgeSwapArgs)]
pub struct PrepareHedgeSwap<'info> {
    pub keeper: Signer<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

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

    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + HedgeBookState::INIT_SPACE,
        seeds = [seeds::HEDGE_BOOK, args.product_program_id.as_ref()],
        bump,
    )]
    pub hedge_book: Box<Account<'info, HedgeBookState>>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + HedgeSleeve::INIT_SPACE,
        seeds = [seeds::HEDGE_SLEEVE, args.product_program_id.as_ref()],
        bump,
    )]
    pub hedge_sleeve: Box<Account<'info, HedgeSleeve>>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + PendingHedgeSwap::INIT_SPACE,
        seeds = [seeds::PENDING_HEDGE_SWAP, args.product_program_id.as_ref()],
        bump,
    )]
    pub pending_hedge_swap: Box<Account<'info, PendingHedgeSwap>>,

    /// CHECK: validated by `halcyon_oracles`.
    pub pyth_sol: UncheckedAccount<'info>,

    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(address = anchor_spl::token::spl_token::native_mint::ID)]
    pub wsol_mint: Box<Account<'info, Mint>>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = usdc_mint,
        associated_token::authority = hedge_sleeve,
    )]
    pub hedge_sleeve_usdc: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = wsol_mint,
        associated_token::authority = hedge_sleeve,
    )]
    pub hedge_sleeve_wsol: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    /// CHECK: sysvar instructions account used to enforce atomic hedge shape.
    #[account(address = anchor_lang::solana_program::sysvar::instructions::ID)]
    pub instructions: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<PrepareHedgeSwap>, args: PrepareHedgeSwapArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.hedge,
        HalcyonError::KeeperAuthorityMismatch
    );
    require!(
        (args.leg_index as usize) < crate::state::hedge_book::MAX_HEDGE_LEGS,
        KernelError::HedgeLegIndexOutOfRange
    );
    require!(
        args.approved_input_amount > 0,
        HalcyonError::BelowMinimumTrade
    );
    // H-1: keeper-supplied slippage must sit under the protocol cap. A
    // compromised keeper key cannot widen slippage beyond
    // `ProtocolConfig.hedge_max_slippage_bps_cap`.
    require!(
        ctx.accounts.protocol_config.hedge_max_slippage_bps_cap > 0
            && args.max_slippage_bps <= ctx.accounts.protocol_config.hedge_max_slippage_bps_cap,
        HalcyonError::SlippageExceeded
    );
    let desired_trade_delta_raw = args
        .target_position_raw
        .checked_sub(args.old_position_raw)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        desired_trade_delta_raw != 0,
        HalcyonError::BelowMinimumTrade
    );
    require!(
        args.min_position_raw <= args.max_position_raw,
        KernelError::InvalidHedgeExecutionBounds
    );
    if desired_trade_delta_raw > 0 {
        require!(
            args.min_position_raw > args.old_position_raw
                && args.max_position_raw <= args.target_position_raw,
            KernelError::InvalidHedgeExecutionBounds
        );
    } else {
        require!(
            args.max_position_raw < args.old_position_raw
                && args.min_position_raw >= args.target_position_raw,
            KernelError::InvalidHedgeExecutionBounds
        );
    }

    let now = Clock::get()?;
    let pyth = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_sol.to_account_info(),
        &halcyon_oracles::feed_ids::SOL_USD,
        &crate::ID,
        &now,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;

    {
        let book = &mut ctx.accounts.hedge_book;
        if book.version == 0 {
            book.version = HedgeBookState::CURRENT_VERSION;
            book.product_program_id = args.product_program_id;
        }
    }
    {
        let sleeve = &mut ctx.accounts.hedge_sleeve;
        if sleeve.version == 0 {
            sleeve.version = HedgeSleeve::CURRENT_VERSION;
            sleeve.product_program_id = args.product_program_id;
        }
    }
    {
        let pending = &mut ctx.accounts.pending_hedge_swap;
        if pending.version == 0 {
            pending.version = PendingHedgeSwap::CURRENT_VERSION;
            pending.product_program_id = args.product_program_id;
        }
    }

    require!(
        !ctx.accounts.pending_hedge_swap.active,
        KernelError::PendingHedgeSwapActive
    );
    require!(
        args.sequence > ctx.accounts.hedge_book.sequence,
        HalcyonError::HedgeSequenceNotMonotonic
    );
    require_keys_eq!(
        ctx.accounts.product_registry_entry.product_program_id,
        ctx.accounts.hedge_book.product_program_id,
        KernelError::HedgeBookProductMismatch
    );
    require_keys_eq!(
        ctx.accounts.product_registry_entry.product_program_id,
        ctx.accounts.hedge_sleeve.product_program_id,
        KernelError::HedgeBookProductMismatch
    );
    require_keys_eq!(
        ctx.accounts.product_registry_entry.product_program_id,
        ctx.accounts.pending_hedge_swap.product_program_id,
        KernelError::HedgeBookProductMismatch
    );

    let pre_usdc_balance = ctx.accounts.hedge_sleeve_usdc.amount;
    let pre_wsol_balance = ctx.accounts.hedge_sleeve_wsol.amount;
    let pre_position_raw =
        i64::try_from(pre_wsol_balance).map_err(|_| error!(HalcyonError::Overflow))?;
    let leg_idx = args.leg_index as usize;
    let prior_book_position = ctx.accounts.hedge_book.legs[leg_idx].current_position_raw;
    require!(
        prior_book_position == args.old_position_raw,
        HalcyonError::HedgeTradeDeltaMismatch
    );
    require!(
        pre_position_raw == args.old_position_raw,
        HalcyonError::HedgeTradeDeltaMismatch
    );

    let source_is_wsol = desired_trade_delta_raw < 0;
    let expected_source_token_account = if source_is_wsol {
        ctx.accounts.hedge_sleeve_wsol.key()
    } else {
        ctx.accounts.hedge_sleeve_usdc.key()
    };
    let expected_destination_token_account = if source_is_wsol {
        ctx.accounts.hedge_sleeve_usdc.key()
    } else {
        ctx.accounts.hedge_sleeve_wsol.key()
    };
    validate_prepare_transaction_shape(
        &ctx.accounts.instructions.to_account_info(),
        args.product_program_id,
        args.sequence,
        ctx.accounts.keeper.key(),
        expected_source_token_account,
        expected_destination_token_account,
    )?;
    let (source_balance_before, destination_balance_before, to_account_info) = if source_is_wsol {
        (
            pre_wsol_balance,
            pre_usdc_balance,
            ctx.accounts.hedge_sleeve_wsol.to_account_info(),
        )
    } else {
        (
            pre_usdc_balance,
            pre_wsol_balance,
            ctx.accounts.hedge_sleeve_usdc.to_account_info(),
        )
    };
    require!(
        source_balance_before >= args.approved_input_amount,
        KernelError::InsufficientHedgeSleeveBalance
    );

    // H-1: bound `approved_input_amount` by the kernel-computed notional
    // envelope for the declared trade delta. A compromised keeper cannot
    // approve more sleeve capital than the declared `target - old`
    // movement requires at `spot × (1 + max_slippage_bps)` pricing, even if
    // the sleeve balance is larger. The envelope is derived from Pyth spot,
    // the kernel's authoritative clock, and the (already-capped)
    // `max_slippage_bps`. We add 1 atomic unit to avoid rounding
    // false-negatives when the declared delta is tiny.
    let trade_abs_raw: u64 = desired_trade_delta_raw.unsigned_abs();
    let approved_ceiling: u64 = if source_is_wsol {
        // Selling SOL: source is WSOL. Max input is |Δposition|.
        trade_abs_raw
    } else {
        // Buying SOL: source is USDC. Max input ≈ |Δposition| × spot × (1+slip).
        let base_notional_raw = reference_notional_usdc_raw(trade_abs_raw, pyth.price_s6)?;
        let slip_num = 10_000u128
            .checked_add(u128::from(args.max_slippage_bps))
            .ok_or(HalcyonError::Overflow)?;
        let ceiling = u128::from(base_notional_raw)
            .checked_mul(slip_num)
            .ok_or(HalcyonError::Overflow)?
            .checked_div(10_000u128)
            .ok_or(HalcyonError::Overflow)?
            .checked_add(1u128)
            .ok_or(HalcyonError::Overflow)?;
        u64::try_from(ceiling).map_err(|_| error!(HalcyonError::Overflow))?
    };
    require!(
        args.approved_input_amount <= approved_ceiling,
        KernelError::InvalidHedgeExecutionBounds
    );
    // Suppress unused-warning on the reserved raw-scale constant; consumers of
    // this module reference it through `reference_notional_usdc_raw`.
    let _ = HEDGE_RAW_SCALE;

    let bump = ctx.bumps.hedge_sleeve;
    let signer_seeds: &[&[&[u8]]] = &[&[
        seeds::HEDGE_SLEEVE,
        args.product_program_id.as_ref(),
        &[bump],
    ]];
    // Approve only the exact hedge input. If finalization never runs, residual
    // delegate authority remains bounded to this one swap amount.
    token::approve(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Approve {
                to: to_account_info,
                delegate: ctx.accounts.keeper.to_account_info(),
                authority: ctx.accounts.hedge_sleeve.to_account_info(),
            },
            signer_seeds,
        ),
        args.approved_input_amount,
    )?;

    let pending = &mut ctx.accounts.pending_hedge_swap;
    pending.active = true;
    pending.keeper = ctx.accounts.keeper.key();
    pending.asset_tag = args.asset_tag;
    pending.leg_index = args.leg_index;
    pending.source_is_wsol = source_is_wsol;
    pending.old_position_raw = args.old_position_raw;
    pending.target_position_raw = args.target_position_raw;
    pending.min_position_raw = args.min_position_raw;
    pending.max_position_raw = args.max_position_raw;
    pending.approved_input_amount = args.approved_input_amount;
    pending.source_balance_before = source_balance_before;
    pending.destination_balance_before = destination_balance_before;
    pending.spot_price_s6 = pyth.price_s6;
    pending.max_slippage_bps = args.max_slippage_bps;
    pending.sequence = args.sequence;
    pending.prepared_at = now.unix_timestamp;

    Ok(())
}

fn validate_prepare_transaction_shape(
    instructions_info: &AccountInfo<'_>,
    product_program_id: Pubkey,
    sequence: u64,
    expected_keeper: Pubkey,
    expected_source_token_account: Pubkey,
    expected_destination_token_account: Pubkey,
) -> Result<()> {
    let current_idx = usize::from(load_current_index_checked(instructions_info)?);
    let jupiter_ix = load_instruction_at_checked(current_idx + 1, instructions_info)
        .map_err(|_| error!(KernelError::InvalidHedgeTransactionShape))?;
    require!(
        jupiter_ix.program_id == JUPITER_V6_PROGRAM_ID,
        KernelError::InvalidHedgeTransactionShape
    );
    validate_jupiter_instruction(
        &jupiter_ix,
        expected_keeper,
        expected_source_token_account,
        expected_destination_token_account,
    )?;

    let record_ix = load_instruction_at_checked(current_idx + 2, instructions_info)
        .map_err(|_| error!(KernelError::InvalidHedgeTransactionShape))?;
    let saw_matching_record = record_ix.program_id == crate::ID
        && record_ix
            .data
            .starts_with(&crate::instruction::RecordHedgeTrade::DISCRIMINATOR)
        && RecordHedgeTradeArgs::try_from_slice(
            &record_ix.data[crate::instruction::RecordHedgeTrade::DISCRIMINATOR.len()..],
        )
        .map(|record_args| {
            record_args.product_program_id == product_program_id && record_args.sequence == sequence
        })
        .map_err(|_| error!(KernelError::InvalidHedgeTransactionShape))?;

    let no_trailing_ix = load_instruction_at_checked(current_idx + 3, instructions_info).is_err();
    require!(
        saw_matching_record && no_trailing_ix,
        KernelError::InvalidHedgeTransactionShape
    );
    Ok(())
}

fn validate_jupiter_instruction(
    ix: &anchor_lang::solana_program::instruction::Instruction,
    expected_keeper: Pubkey,
    expected_source_token_account: Pubkey,
    expected_destination_token_account: Pubkey,
) -> Result<()> {
    let mut saw_keeper_signer = false;
    let mut saw_source = false;
    let mut saw_destination = false;
    for account in &ix.accounts {
        if account.is_signer {
            require!(
                account.pubkey == expected_keeper,
                KernelError::InvalidHedgeTransactionShape
            );
            saw_keeper_signer = true;
        }
        if account.pubkey == expected_source_token_account {
            require!(
                account.is_writable,
                KernelError::InvalidHedgeTransactionShape
            );
            saw_source = true;
        }
        if account.pubkey == expected_destination_token_account {
            require!(
                account.is_writable,
                KernelError::InvalidHedgeTransactionShape
            );
            saw_destination = true;
        }
    }
    require!(
        saw_keeper_signer && saw_source && saw_destination,
        KernelError::InvalidHedgeTransactionShape
    );
    Ok(())
}
