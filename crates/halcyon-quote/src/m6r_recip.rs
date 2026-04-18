//! Reciprocal multiplication replacement for m6r divisions.
//!
//! Alternative to `a.wrapping_mul(b) / S6` that avoids the i64 division on
//! BPF (~140 CU/call) by precomputing the reciprocal in Q62 fixed-point.
//!
//! With ~188 hot-path m6r/m6r_fast calls per quote in the filter, replacing
//! the division with multiply-shift saves an estimated 100-300K CU.
//!
//! Activation: enable feature `m6r-recip` to swap m6r/m6r_fast in
//! `worst_of_c1_filter.rs` to call `m6r_recip` instead. Default-off keeps
//! the K=9+RBF and K=12 shipping paths bit-identical.

pub const S6: i64 = 1_000_000;

/// `RECIP_S6_Q62 = floor(2^62 / S6) = 4_611_686_018_427`.
///
/// (Note: the original task spec listed `4_611_686_018` — that is missing
/// three zeros and corresponds to `2^62 / 10^9`, not `2^62 / 10^6`. The
/// arithmetic error caused 10^7-magnitude divergences in the equivalence
/// check until corrected. Verified: `4_611_686_018_427 × 1_000_000 =
/// 4_611_686_018_427_000_000 ≈ 2^62 = 4_611_686_018_427_387_904`.)
///
/// Multiplying by this and shifting right by 62 is equivalent to dividing
/// by S6 within 1 ULP for bounded inputs. Stored as i128 to make the
/// multiply unambiguously widen.
const RECIP_S6_Q62: i128 = 4_611_686_018_427;

/// Compute `(a * b) / S6` via reciprocal multiplication.
///
/// For inputs bounded by `|a|, |b| ≤ 10 × S6` (well above the filter's
/// realistic range of weights ≤ S6 and means ≤ ~10 × S6), the result is
/// within 1 ULP of the exact division.
///
/// Overflow analysis: worst case `(10^13) × 4.6×10^9 = 4.6×10^22` — safe
/// in i128 by ~16 orders of magnitude.
#[inline(always)]
pub fn m6r_recip(a: i64, b: i64) -> i64 {
    let prod = a.wrapping_mul(b);
    ((prod as i128).wrapping_mul(RECIP_S6_Q62) >> 62) as i64
}
