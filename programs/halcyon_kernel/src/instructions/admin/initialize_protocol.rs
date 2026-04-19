use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct InitializeProtocolArgs {
    pub utilization_cap_bps: u64,
    pub senior_share_bps: u16,
    pub junior_share_bps: u16,
    pub treasury_share_bps: u16,
    pub senior_cooldown_secs: i64,
    pub ewma_rate_limit_secs: i64,
    pub sigma_staleness_cap_secs: i64,
    pub regime_staleness_cap_secs: i64,
    pub regression_staleness_cap_secs: i64,
    pub pyth_quote_staleness_cap_secs: i64,
    pub pyth_settle_staleness_cap_secs: i64,
    pub sigma_floor_annualised_s6: i64,
    /// USDC token account the admin is permitted to sweep fees into. Must be a
    /// USDC account; ownership is enforced at `sweep_fees` time.
    pub treasury_destination: Pubkey,
}

#[derive(Accounts)]
pub struct InitializeProtocol<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = 8 + ProtocolConfig::INIT_SPACE,
        seeds = [seeds::PROTOCOL_CONFIG],
        bump,
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        init,
        payer = admin,
        space = 8 + VaultState::INIT_SPACE,
        seeds = [seeds::VAULT_STATE],
        bump,
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        init,
        payer = admin,
        space = 8 + FeeLedger::INIT_SPACE,
        seeds = [seeds::FEE_LEDGER],
        bump,
    )]
    pub fee_ledger: Account<'info, FeeLedger>,

    #[account(
        init,
        payer = admin,
        space = 8 + KeeperRegistry::INIT_SPACE,
        seeds = [seeds::KEEPER_REGISTRY],
        bump,
    )]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    pub usdc_mint: Account<'info, Mint>,

    /// CHECK: PDA that owns every kernel-side token account.
    #[account(seeds = [seeds::VAULT_AUTHORITY], bump)]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        init,
        payer = admin,
        token::mint = usdc_mint,
        token::authority = vault_authority,
        seeds = [seeds::VAULT_USDC, usdc_mint.key().as_ref()],
        bump,
    )]
    pub vault_usdc: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = admin,
        token::mint = usdc_mint,
        token::authority = vault_authority,
        seeds = [seeds::TREASURY_USDC, usdc_mint.key().as_ref()],
        bump,
    )]
    pub treasury_usdc: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler(ctx: Context<InitializeProtocol>, args: InitializeProtocolArgs) -> Result<()> {
    let clock = Clock::get()?;

    let config = &mut ctx.accounts.protocol_config;
    config.version = ProtocolConfig::CURRENT_VERSION;
    config.admin = ctx.accounts.admin.key();
    config.issuance_paused_global = false;
    config.settlement_paused_global = false;
    config.utilization_cap_bps = args.utilization_cap_bps;
    config.senior_share_bps = args.senior_share_bps;
    config.junior_share_bps = args.junior_share_bps;
    config.treasury_share_bps = args.treasury_share_bps;
    config.senior_cooldown_secs = args.senior_cooldown_secs;
    config.ewma_rate_limit_secs = args.ewma_rate_limit_secs;
    config.sigma_staleness_cap_secs = args.sigma_staleness_cap_secs;
    config.regime_staleness_cap_secs = args.regime_staleness_cap_secs;
    config.regression_staleness_cap_secs = args.regression_staleness_cap_secs;
    config.pyth_quote_staleness_cap_secs = args.pyth_quote_staleness_cap_secs;
    config.pyth_settle_staleness_cap_secs = args.pyth_settle_staleness_cap_secs;
    config.sigma_floor_annualised_s6 = args.sigma_floor_annualised_s6;
    config.k12_correction_sha256 = [0u8; 32];
    config.daily_ki_correction_sha256 = [0u8; 32];
    config.treasury_destination = args.treasury_destination;
    config.last_update_ts = clock.unix_timestamp;

    require!(
        config.premium_splits_sum_to_ten_thousand(),
        crate::KernelError::BadConfig
    );
    // Sanity: treasury_destination cannot be the default pubkey — `sweep_fees`
    // would otherwise reject every call until rotation.
    require_keys_neq!(
        config.treasury_destination,
        Pubkey::default(),
        HalcyonError::DestinationNotAllowed
    );

    let vault = &mut ctx.accounts.vault_state;
    vault.version = VaultState::CURRENT_VERSION;
    vault.last_update_ts = clock.unix_timestamp;
    vault.last_update_slot = clock.slot;

    let fees = &mut ctx.accounts.fee_ledger;
    fees.version = FeeLedger::CURRENT_VERSION;
    fees.last_sweep_ts = clock.unix_timestamp;

    let keepers = &mut ctx.accounts.keeper_registry;
    keepers.version = KeeperRegistry::CURRENT_VERSION;
    keepers.observation = Pubkey::default();
    keepers.regression = Pubkey::default();
    keepers.delta = Pubkey::default();
    keepers.hedge = Pubkey::default();
    keepers.regime = Pubkey::default();
    keepers.last_rotation_ts = clock.unix_timestamp;

    Ok(())
}
