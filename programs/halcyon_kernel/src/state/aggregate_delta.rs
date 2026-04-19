use anchor_lang::prelude::*;

/// Flagship-only: 3D aggregate delta over SPY/QQQ/IWM.
#[account]
#[derive(InitSpace)]
pub struct AggregateDelta {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub delta_spy_s6: i64,
    pub delta_qqq_s6: i64,
    pub delta_iwm_s6: i64,
    pub merkle_root: [u8; 32],
    pub spot_spy_s6: i64,
    pub spot_qqq_s6: i64,
    pub spot_iwm_s6: i64,
    pub live_note_count: u32,
    pub last_update_slot: u64,
    pub last_update_ts: i64,
}

impl AggregateDelta {
    pub const CURRENT_VERSION: u8 = 1;
}
