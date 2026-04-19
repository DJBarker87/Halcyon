//! SOL Autocall pricing and hedge-controller logic.
//!
//! Pure Rust, zero Solana dependencies. Consumed by the
//! `halcyon_sol_autocall` program (L2+) and the backtest harness.

pub mod autocall_hedged;
pub mod autocall_v2;
pub mod autocall_v2_e11;
pub mod autocall_v2_parity;
pub mod capital_stack;
pub mod hedge_controller;
pub mod sol_swap_cost;
