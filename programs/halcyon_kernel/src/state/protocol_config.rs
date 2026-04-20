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
    /// Protocol-level ceiling on `PrepareHedgeSwapArgs.max_slippage_bps`. A
    /// compromised hedge keeper cannot widen slippage beyond this cap.
    /// Expressed in bps (10_000 = 100%). Initialized at
    /// `initialize_protocol` and rotatable via `set_protocol_config`.
    pub hedge_max_slippage_bps_cap: u16,
    /// Allowlisted USDC token account that `defund_hedge_sleeve` may route
    /// to. Pins the admin's sleeve-exit surface the same way
    /// `treasury_destination` pins `sweep_fees` — a single compromised admin
    /// signature cannot exfiltrate sleeve capital to an arbitrary address.
    pub hedge_defund_destination: Pubkey,
    pub last_update_ts: i64,
}

impl ProtocolConfig {
    pub const CURRENT_VERSION: u8 = 4;

    pub fn premium_splits_sum_to_ten_thousand(&self) -> bool {
        self.senior_share_bps as u32 + self.junior_share_bps as u32 + self.treasury_share_bps as u32
            == 10_000
    }

    pub fn sol_autocall_quote_config_valid(&self) -> bool {
        self.sol_autocall_quote_share_bps <= 10_000 && self.sol_autocall_issuer_margin_bps <= 10_000
    }

    pub fn hedge_max_slippage_bps_cap_valid(&self) -> bool {
        self.hedge_max_slippage_bps_cap > 0 && self.hedge_max_slippage_bps_cap <= 10_000
    }

    pub fn premium_vault_portion(&self, premium: u64) -> Option<u64> {
        let premium_u128 = premium as u128;
        let senior_share = premium_u128
            .checked_mul(self.senior_share_bps as u128)?
            .checked_div(10_000u128)? as u64;
        let junior_share = premium_u128
            .checked_mul(self.junior_share_bps as u128)?
            .checked_div(10_000u128)? as u64;
        senior_share.checked_add(junior_share)
    }

    pub fn premium_treasury_share(&self, premium: u64) -> Option<u64> {
        premium.checked_sub(self.premium_vault_portion(premium)?)
    }
}
