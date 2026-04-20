use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct ProductRegistryEntry {
    pub version: u8,
    pub product_program_id: Pubkey,
    pub expected_authority: Pubkey,
    pub active: bool,
    pub paused: bool,
    pub per_policy_risk_cap: u64,
    pub global_risk_cap: u64,
    pub engine_version: u16,
    pub init_terms_discriminator: [u8; 8],
    /// Running per-product sum of `max_liability` across policies currently
    /// holding a reservation (Quoted or Active). Increased by `reserve_and_issue`
    /// and decreased by `apply_settlement` / `reap_quoted`. Gates `global_risk_cap`.
    pub total_reserved: u64,
    /// L3-H1 — when `true` the kernel requires every issuance to escrow at
    /// least `notional` into `vault_usdc` (principal-backed products such
    /// as SOL Autocall). When `false` the product is synthetic (backed by
    /// tranche capital, not buyer principal) and the kernel only demands
    /// the premium-vault share — IL Protection is the first such product.
    /// Set at `register_product` and not mutable via
    /// `update_product_registry`.
    pub requires_principal_escrow: bool,
    pub last_update_ts: i64,
}

impl ProductRegistryEntry {
    pub const CURRENT_VERSION: u8 = 2;
}
