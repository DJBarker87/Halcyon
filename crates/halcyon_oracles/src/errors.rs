use anchor_lang::prelude::*;

#[error_code]
pub enum OracleError {
    #[msg("Pyth feed account has unexpected owner")]
    InvalidOwner,
    #[msg("Pyth feed id does not match the expected asset")]
    FeedIdMismatch,
    #[msg("Pyth price exponent outside supported range")]
    ExponentOutOfRange,
    #[msg("Pyth price verification level insufficient")]
    InsufficientVerification,
    #[msg("oracle price arithmetic overflowed while scaling")]
    ScaleOverflow,
    #[msg("mock-pyth account discriminator mismatch")]
    MockDiscriminatorMismatch,
}
