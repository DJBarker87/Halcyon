use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct ProtocolConfig {
    pub version: u8,
    pub admin: Pubkey,
    pub issuance_paused_global: bool,
    pub settlement_paused_global: bool,
    pub utilization_cap_bps: u64,
    pub senior_share_bps: u16,
    pub junior_share_bps: u16,
    pub treasury_share_bps: u16,
    pub senior_cooldown_secs: i64,
    pub ewma_rate_limit_secs: i64,
    pub sigma_staleness_cap_secs: i64,
    pub regime_staleness_cap_secs: i64,
    pub regression_staleness_cap_secs: i64,
    pub pyth_quote_staleness_cap_secs: i64,
    pub pyth_settle_staleness_cap_secs: i64,
    pub quote_ttl_secs: i64,
    pub sigma_floor_annualised_s6: i64,
    pub sol_autocall_quote_share_bps: u16,
    pub sol_autocall_issuer_margin_bps: u16,
    pub k12_correction_sha256: [u8; 32],
    pub daily_ki_correction_sha256: [u8; 32],
    /// Allowlisted USDC token account that `sweep_fees` may route to. Set
    /// at `initialize_protocol` and rotated via `set_protocol_config`.
    pub treasury_destination: Pubkey,
    pub last_update_ts: i64,
}

impl ProtocolConfig {
    pub const CURRENT_VERSION: u8 = 3;

    pub fn premium_splits_sum_to_ten_thousand(&self) -> bool {
        self.senior_share_bps as u32 + self.junior_share_bps as u32 + self.treasury_share_bps as u32
            == 10_000
    }

    pub fn sol_autocall_quote_config_valid(&self) -> bool {
        self.sol_autocall_quote_share_bps <= 10_000 && self.sol_autocall_issuer_margin_bps <= 10_000
    }
}
