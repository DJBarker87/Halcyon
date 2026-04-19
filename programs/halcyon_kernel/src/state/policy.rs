use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum PolicyStatus {
    Quoted,
    Active,
    Observed,
    AutoCalled,
    KnockedIn,
    Settled,
    Expired,
    Cancelled,
}

#[account]
#[derive(InitSpace)]
pub struct PolicyHeader {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub owner: Pubkey,
    pub notional: u64,
    pub premium_paid: u64,
    pub max_liability: u64,
    pub issued_at: i64,
    pub expiry_ts: i64,
    pub quote_expiry_ts: i64,
    pub settled_at: i64,
    pub terms_hash: [u8; 32],
    pub engine_version: u16,
    pub status: PolicyStatus,
    pub product_terms: Pubkey,
    pub shard_id: u16,
    pub policy_id: Pubkey,
}

impl PolicyHeader {
    pub const CURRENT_VERSION: u8 = 2;
}
