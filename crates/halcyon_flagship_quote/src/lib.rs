//! Flagship worst-of-3 (SPY/QQQ/IWM) autocall pricer.
//!
//! Pure Rust, zero Solana dependencies. Entry point:
//! `worst_of_c1_filter::quote_c1_filter(..., K=12)`, with
//! `k12_correction::k12_correction_lookup(sigma)` applied additively. The
//! `daily_ki_correction` module is scaffolded for the continuous-monitoring
//! correction layered on top in L4.

pub mod b_tensors;
pub mod daily_ki_correction;
pub mod exact_leg_tables;
pub mod frozen_moments_3pt;
pub mod frozen_predict_tables;
pub mod k12_correction;
pub mod k9_correction;
pub mod m6r_recip;
pub mod midlife_pricer;
#[cfg(not(target_os = "solana"))]
pub mod midlife_reference;
pub mod moment_tables;
pub mod nested_grids;
pub mod nig_weights_lookup;
pub mod obs1_seed_tables;
pub mod worst_of_c1_fast;
pub mod worst_of_c1_filter;
pub mod worst_of_c1_filter_gradients;
#[cfg(not(target_os = "solana"))]
pub mod worst_of_c1_lookup;
#[cfg(not(target_os = "solana"))]
pub mod worst_of_factored;
