use anchor_lang::prelude::*;

#[error_code]
pub enum FlagshipAutocallError {
    #[msg("flagship sigma floor is invalid")]
    InvalidSigmaFloor,
    #[msg("flagship sigma is outside the calibrated pricing envelope")]
    SigmaOutOfRange,
    #[msg("flagship quote recomputation mismatch")]
    QuoteRecomputeMismatch,
    #[msg("flagship policy state is invalid")]
    PolicyStateInvalid,
    #[msg("flagship observation index is out of range")]
    ObservationIndexOutOfRange,
    #[msg("flagship observation is not due yet")]
    ObservationNotDue,
    #[msg("flagship observation accounts must be supplied in SPY/QQQ/IWM triplets")]
    ObservationAccountsInvalid,
    #[msg("flagship observation prices are not from a sufficiently synchronized snapshot")]
    ObservationSnapshotSkewed,
    #[msg("flagship coupon or autocall state must be reconciled before closing the policy")]
    ObservationReconciliationRequired,
    #[msg("flagship policy has not reached expiry")]
    PolicyNotExpired,
    #[msg("flagship entry prices must be positive")]
    InvalidEntryPrice,
}
