use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct VaultSigma {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub ewma_var_daily_s12: i128,
    pub ewma_last_ln_ratio_s12: i128,
    pub ewma_last_timestamp: i64,
    pub last_update_slot: u64,
    pub sample_count: u64,
}

impl VaultSigma {
    pub const CURRENT_VERSION: u8 = 1;
}
