use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke_signed,
    },
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};
use halcyon_common::{events::HedgeBookUpdated, seeds, HalcyonError};

use crate::{
    instructions::lifecycle::record_hedge_trade::reference_notional_usdc_raw, state::*, KernelError,
};

const JUPITER_V6_PROGRAM_ID: Pubkey = pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ExecuteHedgeSwapArgs {
    pub product_program_id: Pubkey,
    pub asset_tag: [u8; 8],
    pub leg_index: u8,
    pub old_position_raw: i64,
    pub target_position_raw: i64,
    pub max_slippage_bps: u16,
    pub jupiter_instruction_data: Vec<u8>,
    pub sequence: u64,
}

#[derive(Accounts)]
#[instruction(args: ExecuteHedgeSwapArgs)]
pub struct ExecuteHedgeSwap<'info> {
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

    /// CHECK: Jupiter V6 swap program.
    pub jupiter_program: UncheckedAccount<'info>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, ExecuteHedgeSwap<'info>>,
    args: ExecuteHedgeSwapArgs,
) -> Result<()> {
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
        !ctx.accounts.jupiter_program.key().eq(&Pubkey::default()),
        KernelError::UnexpectedJupiterProgram
    );
    require_keys_eq!(
        ctx.accounts.jupiter_program.key(),
        JUPITER_V6_PROGRAM_ID,
        KernelError::UnexpectedJupiterProgram
    );
    require!(
        !ctx.remaining_accounts.is_empty(),
        KernelError::JupiterAccountsMissing
    );

    let desired_trade_delta_raw = args
        .target_position_raw
        .checked_sub(args.old_position_raw)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        desired_trade_delta_raw != 0,
        HalcyonError::BelowMinimumTrade
    );

    let now = Clock::get()?;
    let pyth = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_sol.to_account_info(),
        &halcyon_oracles::feed_ids::SOL_USD,
        &crate::ID,
        &now,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;

    let pre_usdc_balance = ctx.accounts.hedge_sleeve_usdc.amount;
    let pre_wsol_balance = ctx.accounts.hedge_sleeve_wsol.amount;
    let pre_position_raw =
        i64::try_from(pre_wsol_balance).map_err(|_| error!(HalcyonError::Overflow))?;
    let hedge_sleeve_key = ctx.accounts.hedge_sleeve.key();
    let hedge_sleeve_usdc_key = ctx.accounts.hedge_sleeve_usdc.key();
    let hedge_sleeve_wsol_key = ctx.accounts.hedge_sleeve_wsol.key();

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

    require!(
        remaining_accounts_contain_key(ctx.remaining_accounts, &hedge_sleeve_key),
        KernelError::JupiterAccountsMissing
    );
    require!(
        remaining_accounts_contain_key(ctx.remaining_accounts, &hedge_sleeve_usdc_key),
        KernelError::JupiterAccountsMissing
    );
    require!(
        remaining_accounts_contain_key(ctx.remaining_accounts, &hedge_sleeve_wsol_key),
        KernelError::JupiterAccountsMissing
    );

    let jupiter_accounts = ctx
        .remaining_accounts
        .iter()
        .map(|account| {
            let is_signer = account.is_signer || account.key() == hedge_sleeve_key;
            require!(
                !is_signer
                    || account.key() == hedge_sleeve_key
                    || account.key() == ctx.accounts.keeper.key()
                    || account.key() == ctx.accounts.payer.key(),
                KernelError::UnexpectedJupiterSigner
            );
            Ok(AccountMeta {
                pubkey: account.key(),
                is_signer,
                is_writable: account.is_writable,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let jupiter_ix = Instruction {
        program_id: ctx.accounts.jupiter_program.key(),
        accounts: jupiter_accounts,
        data: args.jupiter_instruction_data.clone(),
    };

    let bump = ctx.bumps.hedge_sleeve;
    let signer_seeds: &[&[&[u8]]] = &[&[
        seeds::HEDGE_SLEEVE,
        args.product_program_id.as_ref(),
        &[bump],
    ]];
    let mut account_infos: Vec<AccountInfo<'info>> =
        Vec::with_capacity(1 + ctx.remaining_accounts.len());
    account_infos.push(ctx.accounts.jupiter_program.to_account_info());
    account_infos.extend(ctx.remaining_accounts.iter().cloned());
    invoke_signed(&jupiter_ix, &account_infos, signer_seeds)?;

    ctx.accounts.hedge_sleeve_usdc.reload()?;
    ctx.accounts.hedge_sleeve_wsol.reload()?;

    let post_usdc_balance = ctx.accounts.hedge_sleeve_usdc.amount;
    let post_wsol_balance = ctx.accounts.hedge_sleeve_wsol.amount;
    let new_position_raw =
        i64::try_from(post_wsol_balance).map_err(|_| error!(HalcyonError::Overflow))?;
    let trade_delta_raw = new_position_raw
        .checked_sub(args.old_position_raw)
        .ok_or(HalcyonError::Overflow)?;
    let actual_usdc_delta_raw = i64::try_from(post_usdc_balance)
        .map_err(|_| error!(HalcyonError::Overflow))?
        .checked_sub(i64::try_from(pre_usdc_balance).map_err(|_| error!(HalcyonError::Overflow))?)
        .ok_or(HalcyonError::Overflow)?;

    if desired_trade_delta_raw > 0 {
        require!(
            trade_delta_raw > 0 && actual_usdc_delta_raw < 0,
            KernelError::InvalidHedgeSwapBalanceDelta
        );
    } else {
        require!(
            trade_delta_raw < 0 && actual_usdc_delta_raw > 0,
            KernelError::InvalidHedgeSwapBalanceDelta
        );
    }

    let position_delta_abs_raw = trade_delta_raw.unsigned_abs();
    let usdc_flow_abs_raw = actual_usdc_delta_raw.unsigned_abs();
    require!(
        position_delta_abs_raw > 0 && usdc_flow_abs_raw > 0,
        KernelError::InvalidHedgeSwapBalanceDelta
    );

    let executed_price_s6 = effective_price_s6(usdc_flow_abs_raw, position_delta_abs_raw)?;
    let executed_slippage_bps = price_deviation_bps(pyth.price_s6, executed_price_s6)?;
    require!(
        executed_slippage_bps <= u64::from(args.max_slippage_bps),
        HalcyonError::SlippageExceeded
    );

    let reference_notional_raw =
        reference_notional_usdc_raw(position_delta_abs_raw, pyth.price_s6)?;
    let execution_cost = usdc_flow_abs_raw.saturating_sub(reference_notional_raw);

    let book = &mut ctx.accounts.hedge_book;
    if leg_idx >= book.leg_count as usize {
        book.leg_count = args.leg_index.saturating_add(1);
    }

    let leg = &mut book.legs[leg_idx];
    leg.asset_tag = args.asset_tag;
    leg.current_position_raw = new_position_raw;
    leg.target_position_raw = args.target_position_raw;
    leg.last_rebalance_ts = now.unix_timestamp;
    leg.last_rebalance_price_s6 = executed_price_s6;

    let sleeve = &mut ctx.accounts.hedge_sleeve;
    sleeve.usdc_reserve = post_usdc_balance;
    sleeve.lifetime_execution_cost = sleeve
        .lifetime_execution_cost
        .checked_add(execution_cost)
        .ok_or(HalcyonError::Overflow)?;
    sleeve.last_update_ts = now.unix_timestamp;

    book.cumulative_execution_cost = book
        .cumulative_execution_cost
        .checked_add(execution_cost)
        .ok_or(HalcyonError::Overflow)?;
    book.last_rebalance_ts = now.unix_timestamp;
    book.sequence = args.sequence;

    emit!(HedgeBookUpdated {
        product_program_id: args.product_program_id,
        hedge_book: book.key(),
        leg_index: args.leg_index,
        new_position_raw,
        trade_delta_raw,
        executed_price_s6,
        updated_at: now.unix_timestamp,
    });
    Ok(())
}

fn remaining_accounts_contain_key(accounts: &[AccountInfo<'_>], key: &Pubkey) -> bool {
    accounts.iter().any(|account| account.key() == *key)
}

fn effective_price_s6(usdc_raw: u64, position_raw: u64) -> Result<i64> {
    require!(usdc_raw > 0, KernelError::InvalidExecutedPrice);
    require!(position_raw > 0, KernelError::InvalidExecutedPrice);

    let raw = u128::from(usdc_raw)
        .checked_mul(1_000_000_000u128)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(u128::from(position_raw))
        .ok_or(HalcyonError::Overflow)?;
    i64::try_from(raw).map_err(|_| error!(HalcyonError::Overflow))
}

fn price_deviation_bps(reference_price_s6: i64, observed_price_s6: i64) -> Result<u64> {
    require!(reference_price_s6 > 0, KernelError::InvalidExecutedPrice);
    require!(observed_price_s6 > 0, KernelError::InvalidExecutedPrice);

    let diff = (i128::from(reference_price_s6) - i128::from(observed_price_s6)).abs() as u128;
    let bps = diff
        .checked_mul(10_000u128)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(
            u128::try_from(reference_price_s6)
                .map_err(|_| error!(KernelError::InvalidExecutedPrice))?,
        )
        .ok_or(HalcyonError::Overflow)?;
    u64::try_from(bps).map_err(|_| error!(HalcyonError::Overflow))
}
