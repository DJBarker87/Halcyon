use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct VaultState {
    pub version: u8,
    pub total_senior: u64,
    pub total_junior: u64,
    pub total_reserved_liability: u64,
    pub lifetime_premium_received: u64,
    pub last_update_slot: u64,
    pub last_update_ts: i64,
}

impl VaultState {
    pub const CURRENT_VERSION: u8 = 1;
}
