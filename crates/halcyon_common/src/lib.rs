//! Halcyon shared primitives.
//!
//! Cross-product surface consumed by both the kernel and every product program:
//! fixed-point scale constants, PDA seeds, overflow-safe conversions,
//! `HalcyonError` enum, and on-chain event schemas.
//!
//! `#![no_std]` is deliberately absent — Anchor's derive macros pull `std` in
//! transitively through `anchor-lang` so the crate cannot be no_std while still
//! exporting `#[error_code]` and `#[event]`-derived types.

pub mod aggregate_delta_signing;
pub mod errors;
pub mod events;
pub mod fixed_point;
pub mod product_ids;
pub mod seeds;

pub use aggregate_delta_signing::{
    encode_aggregate_delta_message, AGGREGATE_DELTA_DOMAIN_TAG, AGGREGATE_DELTA_MESSAGE_LEN,
};
pub use errors::HalcyonError;
pub use fixed_point::{to_scale_12, to_scale_6, SCALE_12, SCALE_6};
pub use product_ids::{FLAGSHIP_AUTOCALL, IL_PROTECTION, SOL_AUTOCALL};
