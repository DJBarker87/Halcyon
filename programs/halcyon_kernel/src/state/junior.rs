use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct JuniorTranche {
    pub version: u8,
    pub owner: Pubkey,
    pub balance: u64,
    pub non_withdrawable: bool,
    pub created_ts: i64,
}

impl JuniorTranche {
    pub const CURRENT_VERSION: u8 = 1;
}
