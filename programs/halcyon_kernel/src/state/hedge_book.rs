use anchor_lang::prelude::*;

/// Up to four hedge legs per product (SPY+QQQ for flagship, SOL for SOL
/// Autocall, one spare). Fixed to keep `InitSpace` static.
pub const MAX_HEDGE_LEGS: usize = 4;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, InitSpace)]
pub struct HedgeLeg {
    pub asset_tag: [u8; 8],
    pub current_position_raw: i64,
    pub target_position_raw: i64,
    pub last_rebalance_ts: i64,
    pub last_rebalance_price_s6: i64,
}

#[account]
#[derive(InitSpace)]
pub struct HedgeBookState {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub leg_count: u8,
    pub legs: [HedgeLeg; MAX_HEDGE_LEGS],
    pub last_aggregate_delta_spot_s6: [i64; MAX_HEDGE_LEGS],
    pub cumulative_execution_cost: u64,
    pub last_rebalance_ts: i64,
    /// Monotonic sequence for kernel-backed hedge executions. Replayed /
    /// reordered keeper writes are rejected by the kernel.
    pub sequence: u64,
}

impl HedgeBookState {
    pub const CURRENT_VERSION: u8 = 1;
}
