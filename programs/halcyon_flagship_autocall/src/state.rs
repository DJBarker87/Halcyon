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
pub const RETAIL_REDEMPTION_NOTICE_SECS: i64 = 48 * 60 * 60;
pub const RETAIL_REDEMPTION_EXPIRY_SECS: i64 = 7 * 24 * 60 * 60;
pub const MIDLIFE_NAV_CHECKPOINT_VERSION: u8 = 1;
pub const MIDLIFE_NAV_CHECKPOINT_EXPIRY_SLOTS: u64 = 512;
pub const MIDLIFE_NAV_CHECKPOINT_K: usize = 15;
pub const MIDLIFE_NAV_CHECKPOINT_MEMORY_BUCKETS: usize = 19;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MidlifeInputSnapshot {
    pub current_spy_s6: i64,
    pub current_qqq_s6: i64,
    pub current_iwm_s6: i64,
    pub sigma_common_s6: i64,
    pub entry_spy_s6: i64,
    pub entry_qqq_s6: i64,
    pub entry_iwm_s6: i64,
    pub beta_spy_s12: i128,
    pub beta_qqq_s12: i128,
    pub alpha_s12: i128,
    pub regression_residual_vol_s6: i64,
    pub monthly_coupon_schedule: [i64; MONTHLY_COUPON_COUNT],
    pub quarterly_autocall_schedule: [i64; QUARTERLY_AUTOCALL_COUNT],
    pub next_coupon_index: u8,
    pub next_autocall_index: u8,
    pub offered_coupon_bps_s6: i64,
    pub coupon_barrier_bps: u16,
    pub autocall_barrier_bps: u16,
    pub ki_barrier_bps: u16,
    pub ki_latched: bool,
    pub missed_coupon_observations: u8,
    pub coupons_paid_usdc: u64,
    pub notional_usdc: u64,
    pub now_trading_day: u16,
}

impl MidlifeInputSnapshot {
    pub const INIT_SPACE: usize = 4 * 8
        + 3 * 8
        + 3 * 16
        + 8
        + MONTHLY_COUPON_COUNT * 8
        + QUARTERLY_AUTOCALL_COUNT * 8
        + 1
        + 1
        + 8
        + 3 * 2
        + 1
        + 1
        + 8
        + 8
        + 2;
}

#[cfg(not(target_os = "solana"))]
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MidlifeCheckpointNode {
    pub c: i64,
    pub w: i64,
    pub mean_u: i64,
    pub mean_v: i64,
}

#[cfg(not(target_os = "solana"))]
impl MidlifeCheckpointNode {
    pub const INIT_SPACE: usize = 4 * 8;
}

#[cfg(not(target_os = "solana"))]
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MidlifeCheckpointState {
    pub nodes: [MidlifeCheckpointNode; MIDLIFE_NAV_CHECKPOINT_K],
    pub n_active: u8,
}

#[cfg(not(target_os = "solana"))]
impl MidlifeCheckpointState {
    pub const INIT_SPACE: usize = MIDLIFE_NAV_CHECKPOINT_K * MidlifeCheckpointNode::INIT_SPACE + 1;
}

#[cfg(not(target_os = "solana"))]
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidlifeNavCheckpointData {
    pub version: u8,
    pub next_coupon_index: u8,
    pub previous_day: i64,
    pub autocall_cursor: u8,
    pub first_coupon_index: u8,
    pub seed_bucket: u8,
    pub redemption_pv_s6: i64,
    pub remaining_coupon_pv_s6: i64,
    pub par_recovery_probability_s6: i64,
    pub safe_states: [MidlifeCheckpointState; MIDLIFE_NAV_CHECKPOINT_MEMORY_BUCKETS],
    pub knocked_states: [MidlifeCheckpointState; MIDLIFE_NAV_CHECKPOINT_MEMORY_BUCKETS],
}

#[cfg(not(target_os = "solana"))]
impl Default for MidlifeNavCheckpointData {
    fn default() -> Self {
        Self {
            version: 0,
            next_coupon_index: 0,
            previous_day: 0,
            autocall_cursor: 0,
            first_coupon_index: 0,
            seed_bucket: 0,
            redemption_pv_s6: 0,
            remaining_coupon_pv_s6: 0,
            par_recovery_probability_s6: 0,
            safe_states: [MidlifeCheckpointState::default(); MIDLIFE_NAV_CHECKPOINT_MEMORY_BUCKETS],
            knocked_states: [MidlifeCheckpointState::default();
                MIDLIFE_NAV_CHECKPOINT_MEMORY_BUCKETS],
        }
    }
}

#[cfg(not(target_os = "solana"))]
impl MidlifeNavCheckpointData {
    pub const INIT_SPACE: usize = 1
        + 1
        + 8
        + 1
        + 1
        + 1
        + 6
        + 3 * 8
        + 2 * MIDLIFE_NAV_CHECKPOINT_MEMORY_BUCKETS * MidlifeCheckpointState::INIT_SPACE
        + 4 * MidlifeCheckpointState::INIT_SPACE;
}

#[cfg(not(target_os = "solana"))]
#[account]
pub struct MidlifeNavCheckpointAccount {
    pub version: u8,
    pub requester: Pubkey,
    pub policy_header: Pubkey,
    pub product_terms: Pubkey,
    pub prepared_slot: u64,
    pub expires_at_slot: u64,
    pub inputs: MidlifeInputSnapshot,
    pub checkpoint: MidlifeNavCheckpointData,
}

#[cfg(not(target_os = "solana"))]
impl MidlifeNavCheckpointAccount {
    pub const INIT_SPACE: usize = 1
        + 3 * 32
        + 8
        + 8
        + MidlifeInputSnapshot::INIT_SPACE
        + MidlifeNavCheckpointData::INIT_SPACE;
}

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

#[account]
#[derive(InitSpace)]
pub struct FlagshipQuoteReceipt {
    pub version: u8,
    pub buyer: Pubkey,
    pub policy_id: Pubkey,
    pub notional_usdc: u64,
    pub premium: u64,
    pub max_liability: u64,
    pub fair_coupon_bps_s6: i64,
    pub offered_coupon_bps_s6: i64,
    pub sigma_pricing_s6: i64,
    pub quote_slot: u64,
    pub entry_spy_price_s6: i64,
    pub entry_qqq_price_s6: i64,
    pub entry_iwm_price_s6: i64,
    pub expiry_ts: i64,
    pub created_at: i64,
    pub beta_spy_s12: i128,
    pub beta_qqq_s12: i128,
    pub alpha_s12: i128,
    pub regression_r_squared_s6: i64,
    pub regression_residual_vol_s6: i64,
}

impl FlagshipQuoteReceipt {
    pub const CURRENT_VERSION: u8 = 1;
}

#[account]
#[derive(InitSpace)]
pub struct RetailRedemptionRequest {
    pub version: u8,
    pub policy_header: Pubkey,
    pub requester: Pubkey,
    pub requested_at: i64,
    pub earliest_execute_ts: i64,
    pub expires_at: i64,
}

impl RetailRedemptionRequest {
    pub const CURRENT_VERSION: u8 = 1;
}
