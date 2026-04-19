use anchor_lang::prelude::*;

pub const OBSERVATION_COUNT: usize = 8;
pub const OBSERVATION_INTERVAL_DAYS: u32 = 2;
pub const MATURITY_DAYS: u32 = 16;
pub const SECONDS_PER_DAY: i64 = 86_400;

pub const AUTOCALL_BARRIER_BPS: u64 = 10_250;
pub const COUPON_BARRIER_BPS: u64 = 10_000;
pub const KI_BARRIER_BPS: u64 = 7_000;
pub const NO_AUTOCALL_FIRST_N_OBS: u8 = 1;

pub const CURRENT_ENGINE_VERSION: u16 = 1;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum ProductStatus {
    Active,
    AutoCalled,
    Settled,
}

#[account]
#[derive(InitSpace)]
pub struct SolAutocallTerms {
    pub version: u8,
    pub policy_header: Pubkey,
    pub entry_price_s6: i64,
    pub autocall_barrier_s6: i64,
    pub coupon_barrier_s6: i64,
    pub ki_barrier_s6: i64,
    /// Scheduled unix timestamps for each of `OBSERVATION_COUNT` observation
    /// dates, relative to `issued_at`. Stored concretely so the keeper and
    /// the observation handler don't need to recompute the schedule.
    pub observation_schedule: [i64; OBSERVATION_COUNT],
    pub no_autocall_first_n_obs: u8,
    pub current_observation_index: u8,
    /// Quoted coupon per observation at SCALE_6 bps. Stored so later
    /// observation / settlement handlers can compute coupon accruals from the
    /// issued terms without rerunning the pricer.
    pub offered_coupon_bps_s6: i64,
    pub quote_share_bps: u16,
    pub issuer_margin_bps: u16,
    /// Cumulative coupons already paid on interim observations, plus the
    /// terminal coupon once the policy has been autocalled or settled.
    pub accumulated_coupon_usdc: u64,
    pub ki_triggered: bool,
    pub status: ProductStatus,
}

impl SolAutocallTerms {
    pub const CURRENT_VERSION: u8 = 1;

    /// Is the `i`th observation the final scheduled observation (maturity)?
    pub fn is_final_observation(&self, i: u8) -> bool {
        i as usize + 1 == OBSERVATION_COUNT
    }
}
