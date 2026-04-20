use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

#[derive(Accounts)]
pub struct WithdrawSenior<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,

    pub usdc_mint: Account<'info, Mint>,

    // L-2 — consistency with `deposit_senior`: the destination must be owned
    // by the signing depositor. The depositor is signing, so this is a UX
    // safety rail, not an authorization check.
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

    /// CHECK: PDA that owns `vault_usdc`; seeds drive signer.
    #[account(seeds = [seeds::VAULT_AUTHORITY], bump)]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut, seeds = [seeds::VAULT_STATE], bump)]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [seeds::SENIOR, depositor.key().as_ref()],
        bump,
        constraint = senior_deposit.owner == depositor.key(),
    )]
    pub senior_deposit: Account<'info, SeniorDeposit>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<WithdrawSenior>, amount: u64) -> Result<()> {
    require!(amount > 0, HalcyonError::BelowMinimumTrade);

    let now = Clock::get()?.unix_timestamp;
    let cooldown = ctx.accounts.protocol_config.senior_cooldown_secs;
    let last_deposit = ctx.accounts.senior_deposit.last_deposit_ts;

    require!(
        now.saturating_sub(last_deposit) >= cooldown,
        HalcyonError::CooldownNotElapsed
    );
    require!(
        ctx.accounts.senior_deposit.balance >= amount,
        KernelError::WithdrawAmountExceedsBalance
    );

    let remaining_senior = ctx
        .accounts
        .vault_state
        .total_senior
        .checked_sub(amount)
        .ok_or(HalcyonError::Overflow)?;
    let remaining_total_capital = remaining_senior
        .checked_add(ctx.accounts.vault_state.total_junior)
        .ok_or(HalcyonError::Overflow)?;
    let reserved_liability = ctx.accounts.vault_state.total_reserved_liability;

    require!(
        remaining_total_capital > 0 || reserved_liability == 0,
        HalcyonError::UtilizationCapExceeded
    );
    if remaining_total_capital > 0 {
        let utilization_bps = (reserved_liability as u128)
            .checked_mul(10_000u128)
            .ok_or(HalcyonError::Overflow)?
            .checked_div(remaining_total_capital as u128)
            .ok_or(HalcyonError::Overflow)? as u64;
        require!(
            utilization_bps <= ctx.accounts.protocol_config.utilization_cap_bps,
            HalcyonError::UtilizationCapExceeded
        );
    }

    let bump = ctx.bumps.vault_authority;
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, &[bump]]];

    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault_usdc.to_account_info(),
                to: ctx.accounts.depositor_usdc.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
    )?;

    let senior = &mut ctx.accounts.senior_deposit;
    senior.balance = senior
        .balance
        .checked_sub(amount)
        .ok_or(HalcyonError::Overflow)?;

    let vault = &mut ctx.accounts.vault_state;
    vault.total_senior = remaining_senior;
    vault.last_update_ts = now;
    Ok(())
}
