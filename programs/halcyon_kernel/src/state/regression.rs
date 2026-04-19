use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
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

impl Regression {
    pub const CURRENT_VERSION: u8 = 1;
}
