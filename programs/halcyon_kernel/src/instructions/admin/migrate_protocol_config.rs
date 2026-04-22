use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, system_instruction};
use halcyon_common::{seeds, HalcyonError};

use crate::{state::ProtocolConfig, KernelError};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
struct ProtocolConfigV4 {
    pub version: u8,
    pub admin: Pubkey,
    pub issuance_paused_global: bool,
    pub settlement_paused_global: bool,
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
    pub quote_ttl_secs: i64,
    pub sigma_floor_annualised_s6: i64,
    pub sol_autocall_quote_share_bps: u16,
    pub sol_autocall_issuer_margin_bps: u16,
    pub k12_correction_sha256: [u8; 32],
    pub daily_ki_correction_sha256: [u8; 32],
    pub treasury_destination: Pubkey,
    pub hedge_max_slippage_bps_cap: u16,
    pub hedge_defund_destination: Pubkey,
    pub last_update_ts: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
struct ProtocolConfigV5 {
    pub version: u8,
    pub admin: Pubkey,
    pub issuance_paused_global: bool,
    pub settlement_paused_global: bool,
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
    pub quote_ttl_secs: i64,
    pub sigma_floor_annualised_s6: i64,
    pub sol_autocall_quote_share_bps: u16,
    pub sol_autocall_issuer_margin_bps: u16,
    pub k12_correction_sha256: [u8; 32],
    pub daily_ki_correction_sha256: [u8; 32],
    pub pod_deim_table_sha256: [u8; 32],
    pub treasury_destination: Pubkey,
    pub hedge_max_slippage_bps_cap: u16,
    pub hedge_defund_destination: Pubkey,
    pub last_update_ts: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
struct ProtocolConfigV6 {
    pub version: u8,
    pub admin: Pubkey,
    pub issuance_paused_global: bool,
    pub settlement_paused_global: bool,
    pub utilization_cap_bps: u64,
    pub senior_share_bps: u16,
    pub junior_share_bps: u16,
    pub treasury_share_bps: u16,
    pub senior_cooldown_secs: i64,
    pub ewma_rate_limit_secs: i64,
    pub il_ewma_rate_limit_secs: u64,
    pub sol_autocall_ewma_rate_limit_secs: u64,
    pub sigma_staleness_cap_secs: i64,
    pub regime_staleness_cap_secs: i64,
    pub regression_staleness_cap_secs: i64,
    pub pyth_quote_staleness_cap_secs: i64,
    pub pyth_settle_staleness_cap_secs: i64,
    pub quote_ttl_secs: i64,
    pub sigma_floor_annualised_s6: i64,
    pub sol_autocall_quote_share_bps: u16,
    pub sol_autocall_issuer_margin_bps: u16,
    pub k12_correction_sha256: [u8; 32],
    pub daily_ki_correction_sha256: [u8; 32],
    pub pod_deim_table_sha256: [u8; 32],
    pub treasury_destination: Pubkey,
    pub hedge_max_slippage_bps_cap: u16,
    pub hedge_defund_destination: Pubkey,
    pub last_update_ts: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
struct ProtocolConfigV7 {
    pub version: u8,
    pub admin: Pubkey,
    pub issuance_paused_global: bool,
    pub settlement_paused_global: bool,
    pub utilization_cap_bps: u64,
    pub senior_share_bps: u16,
    pub junior_share_bps: u16,
    pub treasury_share_bps: u16,
    pub senior_cooldown_secs: i64,
    pub ewma_rate_limit_secs: i64,
    pub il_ewma_rate_limit_secs: u64,
    pub sol_autocall_ewma_rate_limit_secs: u64,
    pub sigma_staleness_cap_secs: i64,
    pub regime_staleness_cap_secs: i64,
    pub regression_staleness_cap_secs: i64,
    pub pyth_quote_staleness_cap_secs: i64,
    pub pyth_settle_staleness_cap_secs: i64,
    pub quote_ttl_secs: i64,
    pub sigma_floor_annualised_s6: i64,
    pub sigma_ceiling_annualised_s6: i64,
    pub sol_autocall_quote_share_bps: u16,
    pub sol_autocall_issuer_margin_bps: u16,
    pub k12_correction_sha256: [u8; 32],
    pub daily_ki_correction_sha256: [u8; 32],
    pub pod_deim_table_sha256: [u8; 32],
    pub treasury_destination: Pubkey,
    pub hedge_max_slippage_bps_cap: u16,
    pub hedge_defund_destination: Pubkey,
    pub last_update_ts: i64,
}

enum LegacyProtocolConfig {
    V4(ProtocolConfigV4),
    V5(ProtocolConfigV5),
    V6(ProtocolConfigV6),
    V7(ProtocolConfigV7),
}

impl LegacyProtocolConfig {
    fn admin(&self) -> Pubkey {
        match self {
            Self::V4(cfg) => cfg.admin,
            Self::V5(cfg) => cfg.admin,
            Self::V6(cfg) => cfg.admin,
            Self::V7(cfg) => cfg.admin,
        }
    }

    fn into_current(self) -> Result<ProtocolConfig> {
        fn per_product_rate_limit(rate_limit_secs: i64) -> Result<u64> {
            u64::try_from(rate_limit_secs).map_err(|_| error!(KernelError::BadConfig))
        }

        const DEFAULT_SIGMA_CEILING_ANNUALISED_S6: i64 = 800_000;
        fn per_product_sigma_floor(sigma_floor_annualised_s6: i64) -> i64 {
            sigma_floor_annualised_s6
        }

        match self {
            Self::V4(legacy) => Ok(ProtocolConfig {
                version: ProtocolConfig::CURRENT_VERSION,
                admin: legacy.admin,
                issuance_paused_global: legacy.issuance_paused_global,
                settlement_paused_global: legacy.settlement_paused_global,
                utilization_cap_bps: legacy.utilization_cap_bps,
                senior_share_bps: legacy.senior_share_bps,
                junior_share_bps: legacy.junior_share_bps,
                treasury_share_bps: legacy.treasury_share_bps,
                senior_cooldown_secs: legacy.senior_cooldown_secs,
                ewma_rate_limit_secs: legacy.ewma_rate_limit_secs,
                il_ewma_rate_limit_secs: per_product_rate_limit(legacy.ewma_rate_limit_secs)?,
                sol_autocall_ewma_rate_limit_secs: per_product_rate_limit(
                    legacy.ewma_rate_limit_secs,
                )?,
                sigma_staleness_cap_secs: legacy.sigma_staleness_cap_secs,
                regime_staleness_cap_secs: legacy.regime_staleness_cap_secs,
                regression_staleness_cap_secs: legacy.regression_staleness_cap_secs,
                pyth_quote_staleness_cap_secs: legacy.pyth_quote_staleness_cap_secs,
                pyth_settle_staleness_cap_secs: legacy.pyth_settle_staleness_cap_secs,
                quote_ttl_secs: legacy.quote_ttl_secs,
                sigma_floor_annualised_s6: legacy.sigma_floor_annualised_s6,
                il_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                sol_autocall_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                flagship_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                sigma_ceiling_annualised_s6: DEFAULT_SIGMA_CEILING_ANNUALISED_S6,
                sol_autocall_quote_share_bps: legacy.sol_autocall_quote_share_bps,
                sol_autocall_issuer_margin_bps: legacy.sol_autocall_issuer_margin_bps,
                k12_correction_sha256: legacy.k12_correction_sha256,
                daily_ki_correction_sha256: legacy.daily_ki_correction_sha256,
                pod_deim_table_sha256: [0u8; 32],
                treasury_destination: legacy.treasury_destination,
                hedge_max_slippage_bps_cap: legacy.hedge_max_slippage_bps_cap,
                hedge_defund_destination: legacy.hedge_defund_destination,
                last_update_ts: legacy.last_update_ts,
            }),
            Self::V5(legacy) => Ok(ProtocolConfig {
                version: ProtocolConfig::CURRENT_VERSION,
                admin: legacy.admin,
                issuance_paused_global: legacy.issuance_paused_global,
                settlement_paused_global: legacy.settlement_paused_global,
                utilization_cap_bps: legacy.utilization_cap_bps,
                senior_share_bps: legacy.senior_share_bps,
                junior_share_bps: legacy.junior_share_bps,
                treasury_share_bps: legacy.treasury_share_bps,
                senior_cooldown_secs: legacy.senior_cooldown_secs,
                ewma_rate_limit_secs: legacy.ewma_rate_limit_secs,
                il_ewma_rate_limit_secs: per_product_rate_limit(legacy.ewma_rate_limit_secs)?,
                sol_autocall_ewma_rate_limit_secs: per_product_rate_limit(
                    legacy.ewma_rate_limit_secs,
                )?,
                sigma_staleness_cap_secs: legacy.sigma_staleness_cap_secs,
                regime_staleness_cap_secs: legacy.regime_staleness_cap_secs,
                regression_staleness_cap_secs: legacy.regression_staleness_cap_secs,
                pyth_quote_staleness_cap_secs: legacy.pyth_quote_staleness_cap_secs,
                pyth_settle_staleness_cap_secs: legacy.pyth_settle_staleness_cap_secs,
                quote_ttl_secs: legacy.quote_ttl_secs,
                sigma_floor_annualised_s6: legacy.sigma_floor_annualised_s6,
                il_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                sol_autocall_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                flagship_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                sigma_ceiling_annualised_s6: DEFAULT_SIGMA_CEILING_ANNUALISED_S6,
                sol_autocall_quote_share_bps: legacy.sol_autocall_quote_share_bps,
                sol_autocall_issuer_margin_bps: legacy.sol_autocall_issuer_margin_bps,
                k12_correction_sha256: legacy.k12_correction_sha256,
                daily_ki_correction_sha256: legacy.daily_ki_correction_sha256,
                pod_deim_table_sha256: legacy.pod_deim_table_sha256,
                treasury_destination: legacy.treasury_destination,
                hedge_max_slippage_bps_cap: legacy.hedge_max_slippage_bps_cap,
                hedge_defund_destination: legacy.hedge_defund_destination,
                last_update_ts: legacy.last_update_ts,
            }),
            Self::V6(legacy) => Ok(ProtocolConfig {
                version: ProtocolConfig::CURRENT_VERSION,
                admin: legacy.admin,
                issuance_paused_global: legacy.issuance_paused_global,
                settlement_paused_global: legacy.settlement_paused_global,
                utilization_cap_bps: legacy.utilization_cap_bps,
                senior_share_bps: legacy.senior_share_bps,
                junior_share_bps: legacy.junior_share_bps,
                treasury_share_bps: legacy.treasury_share_bps,
                senior_cooldown_secs: legacy.senior_cooldown_secs,
                ewma_rate_limit_secs: legacy.ewma_rate_limit_secs,
                il_ewma_rate_limit_secs: legacy.il_ewma_rate_limit_secs,
                sol_autocall_ewma_rate_limit_secs: legacy.sol_autocall_ewma_rate_limit_secs,
                sigma_staleness_cap_secs: legacy.sigma_staleness_cap_secs,
                regime_staleness_cap_secs: legacy.regime_staleness_cap_secs,
                regression_staleness_cap_secs: legacy.regression_staleness_cap_secs,
                pyth_quote_staleness_cap_secs: legacy.pyth_quote_staleness_cap_secs,
                pyth_settle_staleness_cap_secs: legacy.pyth_settle_staleness_cap_secs,
                quote_ttl_secs: legacy.quote_ttl_secs,
                sigma_floor_annualised_s6: legacy.sigma_floor_annualised_s6,
                il_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                sol_autocall_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                flagship_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                sigma_ceiling_annualised_s6: DEFAULT_SIGMA_CEILING_ANNUALISED_S6,
                sol_autocall_quote_share_bps: legacy.sol_autocall_quote_share_bps,
                sol_autocall_issuer_margin_bps: legacy.sol_autocall_issuer_margin_bps,
                k12_correction_sha256: legacy.k12_correction_sha256,
                daily_ki_correction_sha256: legacy.daily_ki_correction_sha256,
                pod_deim_table_sha256: legacy.pod_deim_table_sha256,
                treasury_destination: legacy.treasury_destination,
                hedge_max_slippage_bps_cap: legacy.hedge_max_slippage_bps_cap,
                hedge_defund_destination: legacy.hedge_defund_destination,
                last_update_ts: legacy.last_update_ts,
            }),
            Self::V7(legacy) => Ok(ProtocolConfig {
                version: ProtocolConfig::CURRENT_VERSION,
                admin: legacy.admin,
                issuance_paused_global: legacy.issuance_paused_global,
                settlement_paused_global: legacy.settlement_paused_global,
                utilization_cap_bps: legacy.utilization_cap_bps,
                senior_share_bps: legacy.senior_share_bps,
                junior_share_bps: legacy.junior_share_bps,
                treasury_share_bps: legacy.treasury_share_bps,
                senior_cooldown_secs: legacy.senior_cooldown_secs,
                ewma_rate_limit_secs: legacy.ewma_rate_limit_secs,
                il_ewma_rate_limit_secs: legacy.il_ewma_rate_limit_secs,
                sol_autocall_ewma_rate_limit_secs: legacy.sol_autocall_ewma_rate_limit_secs,
                sigma_staleness_cap_secs: legacy.sigma_staleness_cap_secs,
                regime_staleness_cap_secs: legacy.regime_staleness_cap_secs,
                regression_staleness_cap_secs: legacy.regression_staleness_cap_secs,
                pyth_quote_staleness_cap_secs: legacy.pyth_quote_staleness_cap_secs,
                pyth_settle_staleness_cap_secs: legacy.pyth_settle_staleness_cap_secs,
                quote_ttl_secs: legacy.quote_ttl_secs,
                sigma_floor_annualised_s6: legacy.sigma_floor_annualised_s6,
                il_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                sol_autocall_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                flagship_sigma_floor_annualised_s6: per_product_sigma_floor(
                    legacy.sigma_floor_annualised_s6,
                ),
                sigma_ceiling_annualised_s6: legacy.sigma_ceiling_annualised_s6,
                sol_autocall_quote_share_bps: legacy.sol_autocall_quote_share_bps,
                sol_autocall_issuer_margin_bps: legacy.sol_autocall_issuer_margin_bps,
                k12_correction_sha256: legacy.k12_correction_sha256,
                daily_ki_correction_sha256: legacy.daily_ki_correction_sha256,
                pod_deim_table_sha256: legacy.pod_deim_table_sha256,
                treasury_destination: legacy.treasury_destination,
                hedge_max_slippage_bps_cap: legacy.hedge_max_slippage_bps_cap,
                hedge_defund_destination: legacy.hedge_defund_destination,
                last_update_ts: legacy.last_update_ts,
            }),
        }
    }
}

#[derive(Accounts)]
pub struct MigrateProtocolConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// CHECK: Migrated in place after verifying PDA, owner, discriminator,
    /// and the stored admin pubkey.
    #[account(mut, seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<MigrateProtocolConfig>) -> Result<()> {
    let protocol_config = &ctx.accounts.protocol_config;
    let target_len = 8 + ProtocolConfig::INIT_SPACE;
    let current_len = protocol_config.data_len();

    require!(*protocol_config.owner == crate::ID, KernelError::BadConfig);

    let data = protocol_config.try_borrow_data()?;
    require!(data.len() >= 8, KernelError::BadConfig);
    require!(
        &data[..8] == ProtocolConfig::DISCRIMINATOR,
        KernelError::BadConfig
    );

    if current_len >= target_len {
        let mut slice: &[u8] = &data[8..];
        let cfg =
            ProtocolConfig::deserialize(&mut slice).map_err(|_| error!(KernelError::BadConfig))?;
        require_keys_eq!(
            cfg.admin,
            ctx.accounts.admin.key(),
            HalcyonError::AdminMismatch
        );
        return Ok(());
    }

    let legacy = ProtocolConfigV7::try_from_slice(&data[8..])
        .map(LegacyProtocolConfig::V7)
        .or_else(|_| ProtocolConfigV6::try_from_slice(&data[8..]).map(LegacyProtocolConfig::V6))
        .or_else(|_| ProtocolConfigV5::try_from_slice(&data[8..]).map(LegacyProtocolConfig::V5))
        .or_else(|_| ProtocolConfigV4::try_from_slice(&data[8..]).map(LegacyProtocolConfig::V4))
        .map_err(|_| error!(KernelError::BadConfig))?;
    require_keys_eq!(
        legacy.admin(),
        ctx.accounts.admin.key(),
        HalcyonError::AdminMismatch
    );
    drop(data);

    let rent = Rent::get()?;
    let needed_lamports = rent
        .minimum_balance(target_len)
        .saturating_sub(protocol_config.lamports());
    if needed_lamports > 0 {
        invoke(
            &system_instruction::transfer(
                &ctx.accounts.admin.key(),
                &protocol_config.key(),
                needed_lamports,
            ),
            &[
                ctx.accounts.admin.to_account_info(),
                protocol_config.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
    }

    protocol_config
        .to_account_info()
        .realloc(target_len, true)?;

    let migrated = legacy.into_current()?;

    let mut data = protocol_config.try_borrow_mut_data()?;
    data[..8].copy_from_slice(&ProtocolConfig::DISCRIMINATOR);
    migrated
        .serialize(&mut &mut data[8..])
        .map_err(|_| error!(KernelError::BadConfig))?;
    Ok(())
}
