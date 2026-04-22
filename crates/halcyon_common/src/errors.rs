//! Shared Anchor error codes. Used by the kernel and every product program so
//! error surfaces stay consistent across CPIs.

use anchor_lang::prelude::*;

#[error_code]
pub enum HalcyonError {
    #[msg("arithmetic overflow")]
    Overflow,

    // --- Freshness gates ---
    #[msg("VaultSigma is stale")]
    SigmaStale,
    #[msg("RegimeSignal is stale")]
    RegimeStale,
    #[msg("Regression is stale")]
    RegressionStale,
    #[msg("AutocallSchedule is stale")]
    AutocallScheduleStale,
    #[msg("Pyth price is stale")]
    PythStale,
    #[msg("EWMA update rate limit not elapsed")]
    EwmaRateLimited,

    // --- Pause flags ---
    #[msg("issuance is paused globally")]
    PausedGlobally,
    #[msg("issuance is paused for this product")]
    IssuancePausedPerProduct,
    #[msg("settlement is paused globally")]
    SettlementPausedGlobally,
    #[msg("hedging is paused for this product")]
    HedgingPausedPerProduct,

    // --- Authentication ---
    #[msg("product authority signature missing")]
    ProductAuthoritySignatureMissing,
    #[msg("product authority does not match registry entry")]
    ProductAuthorityMismatch,
    #[msg("admin signature missing")]
    AdminSignatureMissing,
    #[msg("admin pubkey does not match protocol config")]
    AdminMismatch,
    #[msg("keeper authority does not match registry")]
    KeeperAuthorityMismatch,

    // --- Product registry ---
    #[msg("product is paused in the registry")]
    ProductPaused,
    #[msg("product not registered")]
    ProductNotRegistered,

    // --- Capacity ---
    #[msg("vault capacity exceeded")]
    CapacityExceeded,
    #[msg("utilization cap exceeded")]
    UtilizationCapExceeded,
    #[msg("per-policy risk cap exceeded")]
    RiskCapExceeded,

    // --- Slippage / quote integrity ---
    #[msg("slippage bound exceeded")]
    SlippageExceeded,
    #[msg("trade is below the minimum")]
    BelowMinimumTrade,
    #[msg("correction table SHA-256 mismatch")]
    CorrectionTableHashMismatch,

    // --- Lifecycle ---
    #[msg("policy is not in Quoted state")]
    PolicyNotQuoted,
    #[msg("policy is not in Active state")]
    PolicyNotActive,
    #[msg("withdrawal cooldown has not elapsed")]
    CooldownNotElapsed,
    #[msg("policy has not expired")]
    NotExpired,

    // --- Hedge book ---
    #[msg("hedge book rebalance cooldown has not elapsed")]
    HedgeCooldownNotElapsed,
    #[msg("hedge trade delta does not match declared position change")]
    HedgeTradeDeltaMismatch,
    #[msg("hedge book sequence must increase monotonically")]
    HedgeSequenceNotMonotonic,

    // --- Terms binding ---
    #[msg("ProductTerms account is invalid (owner / discriminator / empty)")]
    TermsAccountInvalid,
    #[msg("ProductTerms hash does not match header terms_hash")]
    TermsHashMismatch,

    // --- Global risk cap ---
    #[msg("per-product global risk cap exceeded")]
    GlobalRiskCapExceeded,

    // --- Settlement timing ---
    #[msg("policy has not reached expiry and no force_reason given")]
    ExpiryNotElapsed,
    #[msg("quote expiry is invalid for this protocol")]
    InvalidQuoteExpiry,

    // --- Sweep destination ---
    #[msg("sweep destination does not match configured treasury_destination")]
    DestinationNotAllowed,

    // --- Oracle keeper writes ---
    #[msg("oracle write window must strictly advance previous write")]
    OracleTimestampNotMonotonic,
    #[msg("oracle write rate limit not elapsed")]
    OracleRateLimited,

    // --- CPI origin ---
    #[msg("product authority is not a valid PDA of product_program_id")]
    ProductAuthorityNotPda,

    // --- Reap quoted ---
    #[msg("policy is not reapable (not Quoted or grace not elapsed)")]
    NotReapable,
}
