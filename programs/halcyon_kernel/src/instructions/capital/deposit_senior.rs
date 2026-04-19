use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

#[derive(Accounts)]
pub struct DepositSenior<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = depositor_usdc.mint == usdc_mint.key(),
        constraint = depositor_usdc.owner == depositor.key(),
    )]
    pub depositor_usdc: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        bump,
        constraint = vault_usdc.mint == usdc_mint.key(),
    )]
    pub vault_usdc: Account<'info, TokenAccount>,

    #[account(mut, seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut, seeds = [seeds::VAULT_STATE], bump)]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        init_if_needed,
        payer = depositor,
        space = 8 + SeniorDeposit::INIT_SPACE,
        seeds = [seeds::SENIOR, depositor.key().as_ref()],
        bump,
    )]
    pub senior_deposit: Account<'info, SeniorDeposit>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<DepositSenior>, amount: u64) -> Result<()> {
    require!(amount > 0, HalcyonError::BelowMinimumTrade);

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.depositor_usdc.to_account_info(),
                to: ctx.accounts.vault_usdc.to_account_info(),
                authority: ctx.accounts.depositor.to_account_info(),
            },
        ),
        amount,
    )?;

    let now = Clock::get()?.unix_timestamp;
    let slot = Clock::get()?.slot;

    let senior = &mut ctx.accounts.senior_deposit;
    if senior.version == 0 {
        senior.version = SeniorDeposit::CURRENT_VERSION;
        senior.owner = ctx.accounts.depositor.key();
        senior.created_ts = now;
    }
    senior.balance = senior
        .balance
        .checked_add(amount)
        .ok_or(HalcyonError::Overflow)?;
    senior.last_deposit_ts = now;

    let vault = &mut ctx.accounts.vault_state;
    vault.total_senior = vault
        .total_senior
        .checked_add(amount)
        .ok_or(HalcyonError::Overflow)?;
    vault.last_update_ts = now;
    vault.last_update_slot = slot;
    Ok(())
}
