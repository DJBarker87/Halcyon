use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, Transfer},
};
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

#[derive(Accounts)]
#[instruction(product_program_id: Pubkey)]
pub struct FundCouponVault<'info> {
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
        constraint = admin_usdc.mint == usdc_mint.key(),
        constraint = admin_usdc.owner == admin.key(),
    )]
    pub admin_usdc: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = admin,
        space = 8 + CouponVault::INIT_SPACE,
        seeds = [seeds::COUPON_VAULT, product_program_id.as_ref()],
        bump,
    )]
    pub coupon_vault: Account<'info, CouponVault>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = coupon_vault,
    )]
    pub coupon_vault_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<FundCouponVault>,
    product_program_id: Pubkey,
    amount: u64,
) -> Result<()> {
    require!(amount > 0, HalcyonError::BelowMinimumTrade);

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.admin_usdc.to_account_info(),
                to: ctx.accounts.coupon_vault_usdc.to_account_info(),
                authority: ctx.accounts.admin.to_account_info(),
            },
        ),
        amount,
    )?;

    let now = Clock::get()?.unix_timestamp;
    let coupon_vault = &mut ctx.accounts.coupon_vault;
    if coupon_vault.version == 0 {
        coupon_vault.version = CouponVault::CURRENT_VERSION;
        coupon_vault.product_program_id = product_program_id;
    }
    require_keys_eq!(
        coupon_vault.product_program_id,
        product_program_id,
        crate::KernelError::ProductProgramMismatch
    );
    // L-4 — resync against the canonical ATA balance so direct-donation
    // drift does not silently accumulate. `amount` is the intentional
    // contribution; the ATA balance is the authoritative number.
    ctx.accounts.coupon_vault_usdc.reload()?;
    coupon_vault.usdc_balance = ctx.accounts.coupon_vault_usdc.amount;
    coupon_vault.last_update_ts = now;
    Ok(())
}
