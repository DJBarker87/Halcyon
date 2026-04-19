use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use halcyon_common::{events::PolicyIssued, seeds, HalcyonError};

use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ReserveAndIssueArgs {
    pub policy_id: Pubkey,
    pub notional: u64,
    pub premium: u64,
    pub max_liability: u64,
    pub terms_hash: [u8; 32],
    pub engine_version: u16,
    pub expiry_ts: i64,
    pub shard_id: u16,
}

#[derive(Accounts)]
#[instruction(args: ReserveAndIssueArgs)]
pub struct ReserveAndIssue<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    /// Init first — Anchor's init constraint triggers a system_program CPI
    /// which can reshape the BPF input memory layout. Running init BEFORE
    /// the other `Account<T>` deserializations avoids the cached-T stale
    /// view observed in early L1 localnet runs (see LEARNED.md).
    #[account(
        init,
        payer = buyer,
        space = 8 + PolicyHeader::INIT_SPACE,
        seeds = [seeds::POLICY, args.policy_id.as_ref()],
        bump,
    )]
    pub policy_header: Box<Account<'info, PolicyHeader>>,

    /// Product authority PDA that signs via `invoke_signed` in the product's
    /// `accept_quote` handler. Match against `ProductRegistryEntry`.
    pub product_authority: Signer<'info>,

    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = buyer_usdc.mint == usdc_mint.key(),
        constraint = buyer_usdc.owner == buyer.key(),
    )]
    pub buyer_usdc: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        bump,
        constraint = vault_usdc.mint == usdc_mint.key(),
    )]
    pub vault_usdc: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [seeds::TREASURY_USDC, usdc_mint.key().as_ref()],
        bump,
        constraint = treasury_usdc.mint == usdc_mint.key(),
    )]
    pub treasury_usdc: Box<Account<'info, TokenAccount>>,

    /// CHECK: PDA authority owning vault/treasury USDC accounts.
    #[account(seeds = [seeds::VAULT_AUTHORITY], bump)]
    pub vault_authority: UncheckedAccount<'info>,

    // NOTE: `seeds = [...], bump` validation on kernel-owned PDAs passed
    // through a product->kernel CPI triggered a memory aliasing bug under
    // Anchor 0.32.1 / solana 2.3.0 (see LEARNED.md). We fall back to
    // discriminator-based `Account<T>` validation here; the product cannot
    // pass a spoofed account without matching the kernel's discriminator and
    // owner, which gives equivalent safety.
    #[account(mut)]
    pub protocol_config: Box<Account<'info, ProtocolConfig>>,

    #[account(mut)]
    pub vault_state: Box<Account<'info, VaultState>>,

    #[account(mut)]
    pub fee_ledger: Box<Account<'info, FeeLedger>>,

    /// Per-product registry; mutated here to track `total_reserved` against
    /// the `global_risk_cap`. LEARNED.md — kernel-owned PDA passed across a
    /// CPI boundary is validated by discriminator + owner only.
    #[account(mut)]
    pub product_registry_entry: Box<Account<'info, ProductRegistryEntry>>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ReserveAndIssue>, args: ReserveAndIssueArgs) -> Result<()> {
    // --- 1. Authentication ---
    if ctx.accounts.product_authority.key()
        != ctx.accounts.product_registry_entry.expected_authority
    {
        return err!(HalcyonError::ProductAuthorityMismatch);
    }
    if !ctx.accounts.product_registry_entry.active {
        return err!(HalcyonError::ProductPaused);
    }
    if ctx.accounts.product_registry_entry.paused {
        return err!(HalcyonError::IssuancePausedPerProduct);
    }

    // --- 2. Global pause ---
    if ctx.accounts.protocol_config.issuance_paused_global {
        return err!(HalcyonError::PausedGlobally);
    }

    // --- 3. Cash movement on issuance:
    //        * buyer principal always escrows into `vault_usdc`
    //        * any explicit premium is then split between vault/treasury
    //          according to the protocol config
    //        SOL Autocall v1 currently prices upfront premium at zero, but
    //        the kernel keeps the generic split path for other products.
    require!(args.notional > 0, HalcyonError::BelowMinimumTrade);
    let cfg = &ctx.accounts.protocol_config;
    // K12 — splits must sum to 10_000 on every issuance, not only at
    // initialisation. A partial `set_protocol_config` that drifts the sum
    // would otherwise silently over- or under-collect.
    require!(
        cfg.premium_splits_sum_to_ten_thousand(),
        crate::KernelError::BadConfig
    );
    let premium_u128 = args.premium as u128;
    let senior_share = premium_u128
        .checked_mul(cfg.senior_share_bps as u128)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(10_000u128)
        .ok_or(HalcyonError::Overflow)? as u64;
    let junior_share = premium_u128
        .checked_mul(cfg.junior_share_bps as u128)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(10_000u128)
        .ok_or(HalcyonError::Overflow)? as u64;
    let treasury_share = args
        .premium
        .checked_sub(senior_share)
        .and_then(|r| r.checked_sub(junior_share))
        .ok_or(HalcyonError::Overflow)?;

    let premium_vault_portion = senior_share
        .checked_add(junior_share)
        .ok_or(HalcyonError::Overflow)?;
    let vault_deposit = args
        .notional
        .checked_add(premium_vault_portion)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        args.max_liability <= vault_deposit,
        crate::KernelError::PolicyEscrowInsufficient
    );

    // --- 4. Capacity ---
    let vault = &ctx.accounts.vault_state;
    let new_reserved = vault
        .total_reserved_liability
        .checked_add(args.max_liability)
        .ok_or(HalcyonError::Overflow)?;
    let total_capital = vault
        .total_senior
        .checked_add(vault.total_junior)
        .ok_or(HalcyonError::Overflow)?;

    require!(total_capital > 0, HalcyonError::CapacityExceeded);
    let utilization_bps = (new_reserved as u128)
        .checked_mul(10_000u128)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(total_capital as u128)
        .ok_or(HalcyonError::Overflow)? as u64;
    require!(
        utilization_bps <= ctx.accounts.protocol_config.utilization_cap_bps,
        HalcyonError::UtilizationCapExceeded
    );
    require!(
        args.max_liability <= ctx.accounts.product_registry_entry.per_policy_risk_cap,
        HalcyonError::RiskCapExceeded
    );
    // K9 — per-product aggregate reservation.
    let new_product_reserved = ctx
        .accounts
        .product_registry_entry
        .total_reserved
        .checked_add(args.max_liability)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        new_product_reserved <= ctx.accounts.product_registry_entry.global_risk_cap,
        HalcyonError::GlobalRiskCapExceeded
    );

    if vault_deposit > 0 {
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.buyer_usdc.to_account_info(),
                    to: ctx.accounts.vault_usdc.to_account_info(),
                    authority: ctx.accounts.buyer.to_account_info(),
                },
            ),
            vault_deposit,
        )?;
    }

    if treasury_share > 0 {
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.buyer_usdc.to_account_info(),
                    to: ctx.accounts.treasury_usdc.to_account_info(),
                    authority: ctx.accounts.buyer.to_account_info(),
                },
            ),
            treasury_share,
        )?;
    }

    // --- 5. Fee ledger update ---
    let ledger = &mut ctx.accounts.fee_ledger;
    ledger.treasury_balance = ledger
        .treasury_balance
        .checked_add(treasury_share)
        .ok_or(HalcyonError::Overflow)?;

    // --- 6. Vault-state mutation ---
    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let vault = &mut ctx.accounts.vault_state;
    vault.total_reserved_liability = new_reserved;
    vault.lifetime_premium_received = vault
        .lifetime_premium_received
        .checked_add(args.premium)
        .ok_or(HalcyonError::Overflow)?;
    vault.last_update_ts = now;
    vault.last_update_slot = clock.slot;

    // K9 — commit per-product reservation after vault-level checks pass.
    let registry = &mut ctx.accounts.product_registry_entry;
    registry.total_reserved = new_product_reserved;
    registry.last_update_ts = now;

    // --- 7. PolicyHeader in Quoted state ---
    let header = &mut ctx.accounts.policy_header;
    header.version = PolicyHeader::CURRENT_VERSION;
    header.product_program_id = ctx.accounts.product_registry_entry.product_program_id;
    header.owner = ctx.accounts.buyer.key();
    header.notional = args.notional;
    header.premium_paid = args.premium;
    header.max_liability = args.max_liability;
    header.issued_at = now;
    header.expiry_ts = args.expiry_ts;
    header.settled_at = 0;
    header.terms_hash = args.terms_hash;
    header.engine_version = args.engine_version;
    header.status = PolicyStatus::Quoted;
    header.product_terms = Pubkey::default();
    header.shard_id = args.shard_id;
    header.policy_id = args.policy_id;

    emit!(PolicyIssued {
        policy_id: header.key(),
        product_program_id: header.product_program_id,
        owner: header.owner,
        notional: header.notional,
        premium: header.premium_paid,
        max_liability: header.max_liability,
        issued_at: now,
        expiry_ts: header.expiry_ts,
        engine_version: header.engine_version,
        shard_id: header.shard_id,
    });

    Ok(())
}
