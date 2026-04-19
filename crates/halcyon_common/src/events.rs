//! On-chain events emitted by the kernel (and, from L2 onward, by the product
//! programs). Keepers and the frontend subscribe to these starting in L2, so
//! their wire format is stable — bump the `version` discriminator and add a
//! new event struct rather than silently extending a shipped one.

use anchor_lang::prelude::*;

#[event]
pub struct PolicyIssued {
    pub policy_id: Pubkey,
    pub product_program_id: Pubkey,
    pub owner: Pubkey,
    pub notional: u64,
    pub premium: u64,
    pub max_liability: u64,
    pub issued_at: i64,
    pub expiry_ts: i64,
    pub engine_version: u16,
    pub shard_id: u16,
}

#[event]
pub struct PolicySettled {
    pub policy_id: Pubkey,
    pub product_program_id: Pubkey,
    pub owner: Pubkey,
    pub payout: u64,
    pub reservation_released: u64,
    pub settled_at: i64,
}

#[event]
pub struct CouponPaid {
    pub policy_id: Pubkey,
    pub product_program_id: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    pub remaining_liability: u64,
    pub paid_at: i64,
}

#[event]
pub struct HedgeBookUpdated {
    pub product_program_id: Pubkey,
    pub hedge_book: Pubkey,
    pub leg_index: u8,
    pub new_position_raw: i64,
    pub trade_delta_raw: i64,
    pub executed_price_s6: i64,
    pub updated_at: i64,
}

#[event]
pub struct SleeveFunded {
    pub product_program_id: Pubkey,
    pub hedge_sleeve: Pubkey,
    pub amount: u64,
    pub cumulative_funded_usdc: u64,
    pub funded_at: i64,
}

#[event]
pub struct SleeveDefunded {
    pub product_program_id: Pubkey,
    pub hedge_sleeve: Pubkey,
    pub amount: u64,
    pub cumulative_defunded_usdc: u64,
    pub defunded_at: i64,
}

#[event]
pub struct KeeperRotated {
    pub role: u8,
    pub old_authority: Pubkey,
    pub new_authority: Pubkey,
    pub rotated_at: i64,
}

#[event]
pub struct ConfigUpdated {
    pub admin: Pubkey,
    pub field_tag: u32,
    pub updated_at: i64,
}

#[event]
pub struct FeesSwept {
    pub destination: Pubkey,
    pub amount: u64,
    pub swept_at: i64,
}
