pub mod autocall_hedged;
pub mod autocall_v2;
pub mod autocall_v2_e11;
pub mod autocall_v2_parity;
pub mod capital_stack;
pub mod hedge_controller;
pub mod insurance;
pub mod sol_swap_cost;

// Worst-of-3 (SPY/QQQ/IWM) autocall pricer + shipped correction tables.
// Entry point: `worst_of_c1_filter::quote_c1_filter(..., K=12)`.
// Apply `k12_correction::k12_correction_lookup(σ)` after the quote to
// match on-chain fair_coupon_bps. `daily_ki_correction` is a scaffold
// for the future COS-grid swap; not wired into the production quote
// path yet.
pub mod b_tensors;
pub mod daily_ki_correction;
pub mod exact_leg_tables;
pub mod frozen_moments_3pt;
pub mod frozen_predict_tables;
pub mod k12_correction;
pub mod k9_correction;
pub mod m6r_recip;
pub mod moment_tables;
pub mod nested_grids;
pub mod nig_weights_lookup;
pub mod obs1_seed_tables;
pub mod worst_of_c1_fast;
pub mod worst_of_c1_filter;
pub mod worst_of_c1_filter_gradients;
pub mod worst_of_c1_lookup;
pub mod worst_of_factored;
