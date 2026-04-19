use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct CouponVault {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub usdc_balance: u64,
    pub lifetime_coupons_paid: u64,
    pub last_update_ts: i64,
}

impl CouponVault {
    pub const CURRENT_VERSION: u8 = 1;
}
