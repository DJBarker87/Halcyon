use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use halcyon_common::{events::FeesSwept, seeds, HalcyonError};

use crate::{state::*, KernelError};

#[derive(Accounts)]
pub struct SweepFees<'info> {
    pub admin: Signer<'info>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut, seeds = [seeds::FEE_LEDGER], bump)]
    pub fee_ledger: Account<'info, FeeLedger>,

    #[account(
        mut,
        seeds = [seeds::TREASURY_USDC, usdc_mint.key().as_ref()],
        bump,
        constraint = treasury_usdc.mint == usdc_mint.key(),
    )]
    pub treasury_usdc: Account<'info, TokenAccount>,

    /// CHECK: PDA that owns `treasury_usdc`.
    #[account(seeds = [seeds::VAULT_AUTHORITY], bump)]
    pub vault_authority: UncheckedAccount<'info>,

    /// K5 — destination is pinned to `protocol_config.treasury_destination`.
    /// One compromised admin signature cannot exfiltrate fees to an arbitrary
    /// address in one instruction; the admin must first rotate the destination
    /// via `set_protocol_config` (which emits `ConfigUpdated`) and only then
    /// sweep to it — two observable state changes, not one.
    #[account(
        mut,
        constraint = destination_usdc.mint == usdc_mint.key(),
        constraint = destination_usdc.key() == protocol_config.treasury_destination
            @ HalcyonError::DestinationNotAllowed,
    )]
    pub destination_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<SweepFees>, amount: u64) -> Result<()> {
    require!(
        ctx.accounts.fee_ledger.treasury_balance >= amount,
        KernelError::InsufficientTreasuryBalance
    );

    let bump = ctx.bumps.vault_authority;
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::VAULT_AUTHORITY, &[bump]]];

    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.treasury_usdc.to_account_info(),
                to: ctx.accounts.destination_usdc.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
    )?;

    let now = Clock::get()?.unix_timestamp;
    let ledger = &mut ctx.accounts.fee_ledger;
    ledger.treasury_balance = ledger
        .treasury_balance
        .checked_sub(amount)
        .ok_or(HalcyonError::Overflow)?;
    ledger.last_sweep_ts = now;
    emit!(FeesSwept {
        destination: ctx.accounts.destination_usdc.key(),
        amount,
        swept_at: now,
    });
    Ok(())
}
