use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct HedgeSleeve {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub usdc_reserve: u64,
    pub cumulative_funded_usdc: u64,
    pub cumulative_defunded_usdc: u64,
    pub lifetime_execution_cost: u64,
    pub last_funded_ts: i64,
    pub last_defunded_ts: i64,
    pub last_update_ts: i64,
}

impl HedgeSleeve {
    pub const CURRENT_VERSION: u8 = 2;
}
