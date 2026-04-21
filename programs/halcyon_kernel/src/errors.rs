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
    #[msg("lookup table account is invalid")]
    InvalidLookupTableAccount,
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
    #[msg("Jupiter route requested an unexpected account")]
    UnexpectedJupiterAccount,
    #[msg("declared hedge execution bounds are invalid")]
    InvalidHedgeExecutionBounds,
    #[msg("executed hedge position landed outside declared bounds")]
    ExecutedHedgeOutsideBounds,
    #[msg("hedge sleeve swap produced invalid token balance deltas")]
    InvalidHedgeSwapBalanceDelta,
    #[msg("executed hedge price must be positive")]
    InvalidExecutedPrice,
    #[msg("oracle price must be positive")]
    InvalidOraclePrice,
    #[msg("a hedge swap is already pending reconciliation")]
    PendingHedgeSwapActive,
    #[msg("no pending hedge swap is available to reconcile")]
    PendingHedgeSwapMissing,
    #[msg("pending hedge swap does not match the requested reconciliation")]
    PendingHedgeSwapMismatch,
    #[msg("hedge transaction must be prepare -> Jupiter -> record in one transaction")]
    InvalidHedgeTransactionShape,
    #[msg("hedge sleeve balance is below the approved swap input amount")]
    InsufficientHedgeSleeveBalance,
    #[msg("treasury balance is below requested sweep amount")]
    InsufficientTreasuryBalance,
    #[msg("vault deposit amount is below the premium portion that must remain in the underwriting vault")]
    VaultDepositBelowPremiumPortion,
    #[msg("vault deposit amount is below the required principal escrow for this product")]
    PolicyEscrowInsufficient,
    #[msg("direct kernel Jupiter CPI execution is disabled; use prepare_hedge_swap + record_hedge_trade")]
    DeprecatedHedgeExecutionPath,

    // --- Aggregate delta keeper signature (audit F4b) ---
    #[msg("expected an Ed25519 precompile instruction immediately before write_aggregate_delta")]
    MissingEd25519Instruction,
    #[msg("Ed25519 precompile instruction data is malformed")]
    MalformedEd25519Instruction,
    #[msg("Ed25519 precompile verified a pubkey other than the registered delta keeper")]
    Ed25519PubkeyMismatch,
    #[msg(
        "Ed25519 precompile verified a message different from the canonical aggregate-delta bytes"
    )]
    Ed25519MessageMismatch,

    // --- Aggregate delta Pyth publish_time (audit F2) ---
    #[msg("a Pyth publish_time is older than the configured staleness cap")]
    PythPublishTimeStale,
    #[msg("a Pyth publish_time is in the future beyond the allowed clock skew")]
    PythPublishTimeFuture,
    #[msg("a Pyth publish_time must be strictly monotonic per feed between writes")]
    PythPublishTimeNotMonotonic,

    // --- Aggregate delta IPFS publication (audit F4a) ---
    #[msg(
        "publication_cid is empty; off-chain Merkle artifact must be pinned before on-chain write"
    )]
    PublicationCidEmpty,

    // --- Regime oracle bounds ---
    #[msg("regime fvol must be within the bounded on-chain range")]
    RegimeFvolOutOfRange,

    // --- Regression oracle bounds ---
    #[msg("regression beta is outside the allowed range")]
    RegressionBetaOutOfRange,
    #[msg("regression alpha is outside the allowed range")]
    RegressionAlphaOutOfRange,
    #[msg("regression r-squared must be within [0, 1]")]
    RegressionRSquaredOutOfRange,
    #[msg("regression residual volatility must be non-negative")]
    RegressionResidualVolOutOfRange,
    #[msg("regression sample_count is below the minimum accepted window")]
    RegressionSampleCountTooLow,
}
