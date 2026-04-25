use anchor_lang::prelude::*;

#[error_code]
pub enum IlProtectionError {
    #[msg("entry price from Pyth was non-positive")]
    InvalidEntryPrice,
    #[msg("sigma floor would be non-positive")]
    InvalidSigmaFloor,
    #[msg("quote math failed")]
    QuoteComputationFailed,
    #[msg("policy is not in a state that allows this transition")]
    PolicyStateInvalid,
    #[msg("policy has not reached its expiry timestamp")]
    PolicyNotExpired,
    #[msg("settlement math failed")]
    SettlementComputationFailed,
    #[msg("midlife lending-value computation failed")]
    MidlifePricingFailed,
}
