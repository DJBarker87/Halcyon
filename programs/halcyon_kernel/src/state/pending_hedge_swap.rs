use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct PendingHedgeSwap {
    pub version: u8,
    pub active: bool,
    pub product_program_id: Pubkey,
    pub keeper: Pubkey,
    pub asset_tag: [u8; 8],
    pub leg_index: u8,
    pub source_is_wsol: bool,
    pub old_position_raw: i64,
    pub target_position_raw: i64,
    pub min_position_raw: i64,
    pub max_position_raw: i64,
    pub approved_input_amount: u64,
    pub source_balance_before: u64,
    pub destination_balance_before: u64,
    pub spot_price_s6: i64,
    pub max_slippage_bps: u16,
    pub sequence: u64,
    pub prepared_at: i64,
}

impl PendingHedgeSwap {
    pub const CURRENT_VERSION: u8 = 1;
}
