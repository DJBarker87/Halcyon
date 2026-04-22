//! SOL Autocall pricing and hedge-controller logic.
//!
//! Pure Rust, zero Solana dependencies. Consumed by the
//! `halcyon_sol_autocall` program (L2+) and the backtest harness.

#[cfg(not(target_os = "solana"))]
pub mod autocall_hedged;
pub mod autocall_v2;
pub mod autocall_v2_e11;
#[cfg(not(target_os = "solana"))]
pub mod autocall_v2_parity;
#[cfg(not(target_os = "solana"))]
pub mod capital_stack;
pub mod generated;
#[cfg(not(target_os = "solana"))]
pub mod hedge_controller;
#[cfg(not(target_os = "solana"))]
pub mod sol_swap_cost;
