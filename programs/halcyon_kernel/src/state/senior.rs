use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct SeniorDeposit {
    pub version: u8,
    pub owner: Pubkey,
    pub balance: u64,
    pub accrued_yield: u64,
    pub last_deposit_ts: i64,
    pub created_ts: i64,
}

impl SeniorDeposit {
    pub const CURRENT_VERSION: u8 = 1;
}
