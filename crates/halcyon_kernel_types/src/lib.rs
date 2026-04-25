//! Kernel account layouts exported so product programs can read kernel state
//! without depending on the kernel program crate.
//!
//! # Why layouts live in two places
//!
//! The kernel declares each account with `#[account]`, which binds the layout
//! plus Anchor's 8-byte discriminator to the kernel crate. A product program
//! that depends on `halcyon_kernel` directly would pull the kernel's entire
//! program crate into its BPF binary — a circular-dep / bloat footgun.
//!
//! Instead, this crate holds `AnchorSerialize + AnchorDeserialize` mirror
//! structs with field layouts that match the kernel's `#[account]` structs
//! byte-for-byte. Product programs deserialize raw account data (skipping the
//! 8-byte discriminator) into these mirrors.
//!
//! Keeping two declarations in sync is the price of avoiding the circular
//! dependency. CI enforces parity via `make layouts-check` at the kernel's
//! layer boundary (see `programs/halcyon_kernel/LAYOUTS.md`).

use anchor_lang::prelude::*;

/// Mirror of the kernel's `ProtocolConfig` singleton.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ProtocolConfig {
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
    pub il_sigma_floor_annualised_s6: i64,
    pub sol_autocall_sigma_floor_annualised_s6: i64,
    pub flagship_sigma_floor_annualised_s6: i64,
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

/// Mirror of `ProductRegistryEntry` — one per registered product program.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ProductRegistryEntry {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub expected_authority: Pubkey,
    pub active: bool,
    pub paused: bool,
    pub per_policy_risk_cap: u64,
    pub global_risk_cap: u64,
    pub engine_version: u16,
    pub init_terms_discriminator: [u8; 8],
    pub total_reserved: u64,
    pub last_update_ts: i64,
}

/// Mirror of `VaultSigma` — EWMA state per product.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct VaultSigma {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub oracle_feed_id: [u8; 32],
    pub ewma_var_daily_s12: i128,
    pub ewma_last_ln_ratio_s12: i128,
    pub ewma_last_timestamp: i64,
    pub last_price_s6: i64,
    pub last_publish_ts: i64,
    pub last_publish_slot: u64,
    pub last_update_slot: u64,
    pub sample_count: u64,
}

/// Mirror of `RegimeSignal` — regime-keeper-written fvol + regime enum.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct RegimeSignal {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub fvol_s6: i64,
    /// 0 = calm, 1 = stress.
    pub regime: u8,
    pub sigma_multiplier_s6: i64,
    pub sigma_floor_annualised_s6: i64,
    pub last_update_ts: i64,
    pub last_update_slot: u64,
}

/// Mirror of `Regression` — flagship IWM regression coefficients.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct Regression {
    pub version: u8,
    pub beta_spy_s12: i128,
    pub beta_qqq_s12: i128,
    pub alpha_s12: i128,
    pub r_squared_s6: i64,
    pub residual_vol_s6: i64,
    pub window_start_ts: i64,
    pub window_end_ts: i64,
    pub last_update_slot: u64,
    pub last_update_ts: i64,
    pub sample_count: u32,
}

/// Mirror of `AutocallSchedule` — keeper-posted quarterly observation schedule.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct AutocallSchedule {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub issue_date_ts: i64,
    pub observation_timestamps: [i64; 6],
    pub last_publish_ts: i64,
    pub last_publish_slot: u64,
}

/// Mirror of `CouponSchedule` — keeper-posted monthly coupon observation schedule.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct CouponSchedule {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub issue_date_ts: i64,
    pub observation_timestamps: [i64; 18],
    pub last_publish_ts: i64,
    pub last_publish_slot: u64,
}

/// Mirror of `PolicyHeader` — one per live policy across every product.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct PolicyHeader {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub owner: Pubkey,
    pub notional: u64,
    pub premium_paid: u64,
    pub max_liability: u64,
    pub issued_at: i64,
    pub expiry_ts: i64,
    pub quote_expiry_ts: i64,
    pub settled_at: i64,
    pub terms_hash: [u8; 32],
    pub engine_version: u16,
    pub status: u8,
    pub product_terms: Pubkey,
    pub shard_id: u16,
    pub policy_id: Pubkey,
}

/// Mirror of `KeeperRegistry` — authorised pubkeys per keeper role.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct KeeperRegistry {
    pub version: u8,
    pub observation: Pubkey,
    pub regression: Pubkey,
    pub delta: Pubkey,
    pub hedge: Pubkey,
    pub regime: Pubkey,
    pub last_rotation_ts: i64,
}

/// Canonical status enum for `PolicyHeader.status`.
///
/// Encoded as `u8` on-chain to keep the layout stable across Anchor versions.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PolicyStatus {
    Quoted = 0,
    Active = 1,
    Observed = 2,
    AutoCalled = 3,
    KnockedIn = 4,
    Settled = 5,
    Expired = 6,
    Cancelled = 7,
}

impl PolicyStatus {
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(PolicyStatus::Quoted),
            1 => Some(PolicyStatus::Active),
            2 => Some(PolicyStatus::Observed),
            3 => Some(PolicyStatus::AutoCalled),
            4 => Some(PolicyStatus::KnockedIn),
            5 => Some(PolicyStatus::Settled),
            6 => Some(PolicyStatus::Expired),
            7 => Some(PolicyStatus::Cancelled),
            _ => None,
        }
    }
}
