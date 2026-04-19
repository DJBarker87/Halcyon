use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::get_associated_token_address,
    token::{self, Mint, Token, TokenAccount, Transfer},
};
use halcyon_common::{events::CouponPaid, seeds, HalcyonError};

use crate::{state::*, KernelError};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct PayCouponArgs {
    pub amount: u64,
}

#[derive(Accounts)]
pub struct PayCoupon<'info> {
    pub product_authority: Signer<'info>,

    #[account(mut)]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut)]
    pub vault_state: Account<'info, VaultState>,

    #[account(mut)]
    pub policy_header: Account<'info, PolicyHeader>,

    pub usdc_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = coupon_vault.product_program_id == product_registry_entry.product_program_id
            @ KernelError::ProductProgramMismatch,
    )]
    pub coupon_vault: Account<'info, CouponVault>,

    #[account(
        mut,
        token::mint = usdc_mint,
        token::authority = coupon_vault,
        constraint = coupon_vault_usdc.key()
            == get_associated_token_address(&coupon_vault.key(), &usdc_mint.key())
            @ KernelError::ProductProgramMismatch,
    )]
    pub coupon_vault_usdc: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = buyer_usdc.mint == usdc_mint.key(),
        constraint = buyer_usdc.owner == policy_header.owner @ HalcyonError::ProductAuthorityMismatch,
    )]
    pub buyer_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handler(ctx: Context<PayCoupon>, args: PayCouponArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.product_authority.key(),
        ctx.accounts.product_registry_entry.expected_authority,
        HalcyonError::ProductAuthorityMismatch
    );
    require_keys_eq!(
        ctx.accounts.policy_header.product_program_id,
        ctx.accounts.product_registry_entry.product_program_id,
        KernelError::ProductProgramMismatch
    );
    require!(
        !ctx.accounts.protocol_config.settlement_paused_global,
        HalcyonError::SettlementPausedGlobally
    );
    require!(
        !ctx.accounts.product_registry_entry.paused,
        HalcyonError::IssuancePausedPerProduct
    );
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Active,
        HalcyonError::PolicyNotActive
    );

    if args.amount == 0 {
        return Ok(());
    }

    let product_program_id = ctx.accounts.product_registry_entry.product_program_id;
    let (expected_coupon_vault, bump) = Pubkey::find_program_address(
        &[seeds::COUPON_VAULT, product_program_id.as_ref()],
        &crate::ID,
    );
    require_keys_eq!(
        ctx.accounts.coupon_vault.key(),
        expected_coupon_vault,
        KernelError::ProductProgramMismatch
    );
    let signer_seeds: &[&[&[u8]]] = &[&[seeds::COUPON_VAULT, product_program_id.as_ref(), &[bump]]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.coupon_vault_usdc.to_account_info(),
                to: ctx.accounts.buyer_usdc.to_account_info(),
                authority: ctx.accounts.coupon_vault.to_account_info(),
            },
            signer_seeds,
        ),
        args.amount,
    )?;

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;

    let coupon_vault = &mut ctx.accounts.coupon_vault;
    coupon_vault.usdc_balance = coupon_vault
        .usdc_balance
        .checked_sub(args.amount)
        .ok_or(HalcyonError::Overflow)?;
    coupon_vault.lifetime_coupons_paid = coupon_vault
        .lifetime_coupons_paid
        .checked_add(args.amount)
        .ok_or(HalcyonError::Overflow)?;
    coupon_vault.last_update_ts = now;

    let header = &ctx.accounts.policy_header;

    emit!(CouponPaid {
        policy_id: header.key(),
        product_program_id: header.product_program_id,
        owner: header.owner,
        amount: args.amount,
        remaining_liability: header.max_liability,
        paid_at: now,
    });
    Ok(())
}
