use anchor_lang::prelude::*;

pub const COUPON_OBSERVATION_COUNT: usize = 18;

#[account]
#[derive(InitSpace)]
pub struct CouponSchedule {
    pub version: u8,
    pub product_program_id: Pubkey,
    /// Keeper-supplied reference timestamp used to generate this schedule.
    pub issue_date_ts: i64,
    pub observation_timestamps: [i64; COUPON_OBSERVATION_COUNT],
    pub last_publish_ts: i64,
    pub last_publish_slot: u64,
}

impl CouponSchedule {
    pub const CURRENT_VERSION: u8 = 1;
}
