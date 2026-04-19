use anchor_lang::prelude::*;

/// Kernel-local error surface. Shared errors live in `halcyon_common::HalcyonError`
/// — those cross program boundaries; these do not.
#[error_code]
pub enum KernelError {
    #[msg("premium split BPS do not sum to 10000")]
    BadConfig,
    #[msg("product already registered")]
    ProductAlreadyRegistered,
    #[msg("lookup table registry is full")]
    LookupTableRegistryFull,
    #[msg("lookup table index out of range")]
    LookupTableIndexOutOfRange,
    #[msg("invalid keeper role tag")]
    InvalidKeeperRole,
    #[msg("fee ledger has no bucket for this product")]
    FeeBucketMissing,
    #[msg("fee ledger is full")]
    FeeLedgerFull,
    #[msg("withdraw amount exceeds senior balance")]
    WithdrawAmountExceedsBalance,
    #[msg("premium recomputation mismatch")]
    PremiumMismatch,
    #[msg("settlement payout exceeds reserved max_liability")]
    PayoutExceedsMaxLiability,
    #[msg("product program id mismatch on policy header")]
    ProductProgramMismatch,
    #[msg("hedge leg index out of range")]
    HedgeLegIndexOutOfRange,
    #[msg("hedge book product_program_id does not match registry")]
    HedgeBookProductMismatch,
    #[msg("unexpected Jupiter program")]
    UnexpectedJupiterProgram,
    #[msg("Jupiter swap accounts were not supplied")]
    JupiterAccountsMissing,
    #[msg("Jupiter requested an unexpected signer account")]
    UnexpectedJupiterSigner,
    #[msg("hedge sleeve swap produced invalid token balance deltas")]
    InvalidHedgeSwapBalanceDelta,
    #[msg("executed hedge price must be positive")]
    InvalidExecutedPrice,
    #[msg("treasury balance is below requested sweep amount")]
    InsufficientTreasuryBalance,
    #[msg("reserved max_liability exceeds escrow deposited into the underwriting vault")]
    PolicyEscrowInsufficient,
    #[msg("manual hedge trade recording is disabled; use execute_hedge_swap")]
    HedgeTradeRecordingDisabled,
}
