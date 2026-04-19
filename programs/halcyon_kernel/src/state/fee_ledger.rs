use anchor_lang::prelude::*;

pub const MAX_FEE_BUCKETS: usize = 8;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, InitSpace)]
pub struct FeeBucket {
    pub product_program_id: Pubkey,
    pub accrued_usdc: u64,
}

#[account]
#[derive(InitSpace)]
pub struct FeeLedger {
    pub version: u8,
    pub treasury_balance: u64,
    pub bucket_count: u8,
    pub buckets: [FeeBucket; MAX_FEE_BUCKETS],
    pub last_sweep_ts: i64,
}

impl FeeLedger {
    pub const CURRENT_VERSION: u8 = 1;
}
