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
