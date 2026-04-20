use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use halcyon_common::{events::SleeveDefunded, seeds, HalcyonError};

use crate::state::*;

const HEDGE_SLEEVE_DEFUND_COOLDOWN_SECS: i64 = 86_400;

#[derive(Accounts)]
#[instruction(product_program_id: Pubkey)]
pub struct DefundHedgeSleeve<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
        has_one = admin @ HalcyonError::AdminMismatch,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, product_program_id.as_ref()],
        bump,
        constraint = product_registry_entry.product_program_id == product_program_id
            @ crate::KernelError::ProductProgramMismatch,
        constraint = product_registry_entry.active @ HalcyonError::ProductNotRegistered,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    #[account(
        mut,
        seeds = [seeds::HEDGE_SLEEVE, product_program_id.as_ref()],
        bump,
        constraint = hedge_sleeve.product_program_id == product_program_id
            @ crate::KernelError::ProductProgramMismatch,
    )]
    pub hedge_sleeve: Account<'info, HedgeSleeve>,

    #[account(
        mut,
        constraint = hedge_sleeve_usdc.mint == usdc_mint.key(),
        constraint = hedge_sleeve_usdc.owner == hedge_sleeve.key()
            @ crate::KernelError::ProductProgramMismatch,
    )]
    pub hedge_sleeve_usdc: Account<'info, TokenAccount>,

    /// M-2 — destination is pinned to `protocol_config.hedge_defund_destination`.
    /// A compromised admin signature cannot route sleeve capital to an
    /// arbitrary address in one instruction; the admin must first rotate the
    /// destination via `set_protocol_config` (emits `ConfigUpdated`) and only
    /// then defund — two observable state changes, not one.
    #[account(
        mut,
        constraint = destination_usdc.mint == usdc_mint.key(),
        constraint = destination_usdc.key() == protocol_config.hedge_defund_destination
            @ HalcyonError::DestinationNotAllowed,
    )]
    pub destination_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(
    ctx: Context<DefundHedgeSleeve>,
    product_program_id: Pubkey,
    amount: u64,
) -> Result<()> {
    require!(amount > 0, HalcyonError::BelowMinimumTrade);

    // M-2 — cooldown applies to every call, including the first. A compromised
    // admin key cannot drain the sleeve in one transaction immediately after
    // funding; at least `HEDGE_SLEEVE_DEFUND_COOLDOWN_SECS` must elapse since
    // the more recent of the last fund or last defund.
    let now = Clock::get()?.unix_timestamp;
    let last_defund_ts = ctx.accounts.hedge_sleeve.last_defunded_ts;
    let last_fund_ts = ctx.accounts.hedge_sleeve.last_funded_ts;
    let gate_ts = last_defund_ts.max(last_fund_ts);
    require!(
        gate_ts > 0 && now.saturating_sub(gate_ts) >= HEDGE_SLEEVE_DEFUND_COOLDOWN_SECS,
        HalcyonError::CooldownNotElapsed
    );

    let bump = ctx.bumps.hedge_sleeve;
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::HEDGE_SLEEVE, product_program_id.as_ref(), &[bump]]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.hedge_sleeve_usdc.to_account_info(),
                to: ctx.accounts.destination_usdc.to_account_info(),
                authority: ctx.accounts.hedge_sleeve.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
    )?;

    ctx.accounts.hedge_sleeve_usdc.reload()?;
    let hedge_sleeve = &mut ctx.accounts.hedge_sleeve;
    hedge_sleeve.usdc_reserve = ctx.accounts.hedge_sleeve_usdc.amount;
    hedge_sleeve.cumulative_defunded_usdc = hedge_sleeve
        .cumulative_defunded_usdc
        .checked_add(amount)
        .ok_or(HalcyonError::Overflow)?;
    hedge_sleeve.last_defunded_ts = now;
    hedge_sleeve.last_update_ts = now;

    emit!(SleeveDefunded {
        product_program_id,
        hedge_sleeve: hedge_sleeve.key(),
        amount,
        cumulative_defunded_usdc: hedge_sleeve.cumulative_defunded_usdc,
        defunded_at: now,
    });
    Ok(())
}
