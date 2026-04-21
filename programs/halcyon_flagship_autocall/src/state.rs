use anchor_lang::prelude::*;

pub const MONTHLY_COUPON_COUNT: usize = 18;
pub const QUARTERLY_AUTOCALL_COUNT: usize = 6;
#[cfg(feature = "integration-test")]
pub const SECONDS_PER_DAY: i64 = 1;
#[cfg(not(feature = "integration-test"))]
pub const SECONDS_PER_DAY: i64 = 86_400;
#[cfg(feature = "integration-test")]
pub const TENOR_TRADING_DAYS: u16 = 18;
#[cfg(not(feature = "integration-test"))]
pub const TENOR_TRADING_DAYS: u16 = 378;
#[cfg(feature = "integration-test")]
pub const MONTHLY_COUPON_TRADING_DAY_BOUNDARIES: [u16; MONTHLY_COUPON_COUNT] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
];
#[cfg(not(feature = "integration-test"))]
pub const MONTHLY_COUPON_TRADING_DAY_BOUNDARIES: [u16; MONTHLY_COUPON_COUNT] = [
    21, 42, 63, 84, 105, 126, 147, 168, 189, 210, 231, 252, 273, 294, 315, 336, 357, 378,
];
#[cfg(feature = "integration-test")]
pub const QUARTERLY_AUTOCALL_TRADING_DAY_BOUNDARIES: [u16; QUARTERLY_AUTOCALL_COUNT] =
    [3, 6, 9, 12, 15, 18];
#[cfg(not(feature = "integration-test"))]
pub const QUARTERLY_AUTOCALL_TRADING_DAY_BOUNDARIES: [u16; QUARTERLY_AUTOCALL_COUNT] =
    [63, 126, 189, 252, 315, 378];

pub const COUPON_BARRIER_BPS: u16 = 10_000;
pub const AUTOCALL_BARRIER_BPS: u16 = 10_000;
pub const KI_BARRIER_BPS: u16 = 8_000;
pub const CURRENT_ENGINE_VERSION: u16 = 1;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
#[repr(u8)]
pub enum ProductStatus {
    Active = 0,
    AutoCalled = 1,
    Settled = 2,
}

#[account]
#[derive(InitSpace)]
pub struct FlagshipAutocallTerms {
    pub version: u8,
    pub policy_header: Pubkey,
    pub entry_spy_price_s6: i64,
    pub entry_qqq_price_s6: i64,
    pub entry_iwm_price_s6: i64,
    pub monthly_coupon_schedule: [i64; MONTHLY_COUPON_COUNT],
    pub quarterly_autocall_schedule: [i64; QUARTERLY_AUTOCALL_COUNT],
    pub next_coupon_index: u8,
    pub next_autocall_index: u8,
    pub offered_coupon_bps_s6: i64,
    pub coupon_barrier_bps: u16,
    pub autocall_barrier_bps: u16,
    pub ki_barrier_bps: u16,
    pub missed_coupon_observations: u8,
    pub ki_latched: bool,
    pub coupons_paid_usdc: u64,
    pub beta_spy_s12: i128,
    pub beta_qqq_s12: i128,
    pub alpha_s12: i128,
    pub regression_r_squared_s6: i64,
    pub regression_residual_vol_s6: i64,
    pub k12_correction_sha256: [u8; 32],
    pub daily_ki_correction_sha256: [u8; 32],
    pub settled_payout_usdc: u64,
    pub settled_at: i64,
    pub status: ProductStatus,
}

impl FlagshipAutocallTerms {
    pub const CURRENT_VERSION: u8 = 1;
}
