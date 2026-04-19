use anchor_lang::prelude::*;

pub const MAX_LOOKUP_TABLES: usize = 4;

#[account]
#[derive(InitSpace)]
pub struct LookupTableRegistry {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub count: u8,
    pub tables: [Pubkey; MAX_LOOKUP_TABLES],
    pub last_update_ts: i64,
}

impl LookupTableRegistry {
    pub const CURRENT_VERSION: u8 = 1;
}
