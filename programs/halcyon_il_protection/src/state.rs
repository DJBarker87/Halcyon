use anchor_lang::prelude::*;

pub const CURRENT_ENGINE_VERSION: u16 = halcyon_il_quote::CURRENT_ENGINE_VERSION;
pub const TENOR_DAYS: u32 = halcyon_il_quote::TENOR_DAYS;
pub const SECONDS_PER_DAY: i64 = 86_400;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
#[repr(u8)]
pub enum PoolKind {
    RaydiumSolUsdcCpmm = 0,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
#[repr(u8)]
pub enum IssuedRegime {
    Calm = 0,
    Stress = 1,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
#[repr(u8)]
pub enum ProductStatus {
    Active = 0,
    Settled = 1,
}

#[account]
#[derive(InitSpace)]
pub struct IlProtectionTerms {
    pub version: u8,
    pub policy_header: Pubkey,
    pub pool_kind: PoolKind,
    pub weight_s12: u64,
    pub deductible_s6: i64,
    pub cap_s6: i64,
    pub entry_sol_price_s6: i64,
    pub entry_usdc_price_s6: i64,
    pub insured_notional_usdc: u64,
    pub expiry_ts: i64,
    pub fvol_s6: i64,
    pub regime: IssuedRegime,
    pub sigma_multiplier_s6: i64,
    pub sigma_floor_annualised_s6: i64,
    pub sigma_pricing_s6: i64,
    pub settled_terminal_il_s12: u128,
    pub settled_payout_usdc: u64,
    pub settled_at: i64,
    pub status: ProductStatus,
}

impl IlProtectionTerms {
    pub const CURRENT_VERSION: u8 = 1;
}
