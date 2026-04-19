//! Fixed-point scale constants and overflow-safe conversions used at the
//! product-handler / quote-crate boundary per seam 3.1 of
//! `integration_architecture.md`.

use crate::errors::HalcyonError;
use anchor_lang::prelude::*;

/// USDC and natural-token amounts use a 1e6 fixed-point scale.
pub const SCALE_6: i64 = 1_000_000;

/// Ratio-scale intermediates (entry/exit price ratios) use 1e12 throughout
/// the existing pricer surface and the wasm shim.
pub const SCALE_12: i128 = 1_000_000_000_000;

/// Convert an unsigned native-token amount (e.g. USDC base units) into the
/// signed i64 SCALE_6 representation the quote crates consume.
///
/// Fails with `HalcyonError::Overflow` if the value exceeds `i64::MAX`.
#[inline]
pub fn to_scale_6(amount: u64) -> Result<i64> {
    i64::try_from(amount).map_err(|_| error!(HalcyonError::Overflow))
}

/// Convert an unsigned native-token amount into the wider i128 SCALE_12
/// representation used by ratio intermediates and tail premium accumulation.
///
/// The SCALE_12 result is `amount * SCALE_12 / SCALE_6`, which keeps a u64
/// premium well inside i128 range.
#[inline]
pub fn to_scale_12(amount: u64) -> Result<i128> {
    (amount as i128)
        .checked_mul(SCALE_12 / SCALE_6 as i128)
        .ok_or_else(|| error!(HalcyonError::Overflow))
}
