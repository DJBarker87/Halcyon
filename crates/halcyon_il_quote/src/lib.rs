//! IL Protection pricing for SOL/USDC CPMM positions.
//!
//! Pure Rust, zero Solana dependencies. The production engine is
//! `insurance::european_nig::nig_european_il_premium`; settlement runs
//! `insurance::settlement::compute_settlement_from_prices` against Pyth
//! entry/exit snapshots.
//!
//! `src/pool/` holds LP-math helpers reserved for a possible LP-path
//! variant; these files are intentionally NOT declared as a module here.
//! They reference a `halcyon_common` sub-surface (`fp`, `fees`, `constants`)
//! that does not yet exist — they were never wired into the v1 synthetic
//! path and carry over as reference-only until the LP-path scope reopens.

pub mod insurance;
pub mod midlife;
pub mod product;

pub use product::{
    classify_regime_from_fvol_s6, compute_fvol_from_daily_closes, price_il_protection,
    IlProtectionQuote, RegimeConfig, RegimeKind, CAP_S12, CAP_S6, CURRENT_ENGINE_VERSION,
    DEDUCTIBLE_S12, DEDUCTIBLE_S6, FVOL_STRESS_THRESHOLD_S6, MAX_PAYOUT_FRACTION_S6, NIG_ALPHA_S6,
    NIG_BETA_S6, POOL_WEIGHT_S12, SIGMA_FLOOR_ANNUALISED_S6, SIGMA_MULTIPLIER_CALM_S6,
    SIGMA_MULTIPLIER_STRESS_S6, TENOR_DAYS, UNDERWRITING_LOAD_S6,
};
