use anchor_lang::prelude::*;

#[error_code]
pub enum SolAutocallError {
    #[msg("Richardson cross-check gap exceeded the confidence threshold; pricing aborted")]
    PriceConfidenceLow,
    #[msg("fair coupon is below the SOL Autocall issuance floor")]
    FairCouponBelowIssuanceFloor,
    #[msg("quote parameters did not match the recomputed values within tolerance")]
    QuoteRecomputeMismatch,
    #[msg("entry price from Pyth was non-positive")]
    InvalidEntryPrice,
    #[msg("observation too early: scheduled time has not elapsed")]
    ObservationNotDue,
    #[msg("observation index is out of range for this policy")]
    ObservationIndexOutOfRange,
    #[msg("observation would skip already-recorded index; no-op")]
    ObservationAlreadyRecorded,
    #[msg("policy is not in a state that allows this transition")]
    PolicyStateInvalid,
    #[msg("policy has not reached its expiry timestamp")]
    PolicyNotExpired,
    #[msg("observation schedule length mismatch")]
    InvalidSchedule,
    #[msg("sigma floor would be non-positive")]
    InvalidSigmaFloor,
    #[msg("terms_hash recompute does not match stored header hash")]
    TermsHashMismatch,
    #[msg("keeper authority does not match observation role")]
    ObservationKeeperMismatch,
    #[msg("EWMA to sigma composition failed")]
    SigmaCompositionUnavailable,
    #[msg("keeper-fed reduced operators are stale or unavailable for the current sigma")]
    ReducedOperatorsStale,
    #[msg("keeper-fed reduced operator payload has the wrong shape")]
    ReducedOperatorsShapeInvalid,
    #[msg("keeper-fed reduced operator payload exceeds the proven safe bound")]
    ReducedOperatorsRangeInvalid,
    #[msg("keeper-fed reduced operator chunk start does not match the current upload offset")]
    ReducedOperatorsOffsetInvalid,
    #[msg(
        "keeper-fed reduced operator upload state does not match the current sigma or source slots"
    )]
    ReducedOperatorsUploadStateInvalid,
    #[msg("pricing sigma is outside the supported keeper-fed POD-DEIM band")]
    PricingSigmaOutOfBand,
}
