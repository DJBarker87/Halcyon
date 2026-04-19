use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
#[repr(u8)]
pub enum Regime {
    Calm = 0,
    Stress = 1,
}

#[account]
#[derive(InitSpace)]
pub struct RegimeSignal {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub fvol_s6: i64,
    pub regime: Regime,
    pub sigma_multiplier_s6: i64,
    pub sigma_floor_annualised_s6: i64,
    pub last_update_ts: i64,
    pub last_update_slot: u64,
}

impl RegimeSignal {
    pub const CURRENT_VERSION: u8 = 1;
}
