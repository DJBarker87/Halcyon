//! SOL Autocall product program — L2.
//!
//! Four public instructions:
//! - `preview_quote`   — read-only, simulateTransaction-driven.
//! - `accept_quote`    — full issuance path (kernel mutual CPI, confidence gate).
//! - `record_observation` — keeper-driven per-observation handler.
//! - `settle`          — maturity settlement (anyone can call after expiry).
//!
//! CPI note: per LEARNED.md L1, this program deliberately passes kernel-owned
//! PDAs through without the `seeds + bump` constraints Anchor 0.32.1 has a
//! known aliasing bug against. Discriminator-based `Account<T>` validation is
//! the only check; product-side accounts reading kernel PDAs are unaffected
//! because they are not passed across a CPI boundary.

use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod pricing;
pub mod state;

pub use errors::SolAutocallError;
// L-7 — same Anchor macro-expansion constraint as in the kernel's
// `instructions/*` mod.rs files. See notes there.
#[allow(ambiguous_glob_reexports)]
pub use instructions::accept_quote::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::preview_quote::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::record_observation::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::settle::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::write_reduced_operators::*;
pub use state::*;

declare_id!("6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8");

#[program]
pub mod halcyon_sol_autocall {
    use super::*;

    pub fn preview_quote(ctx: Context<PreviewQuote>, notional_usdc: u64) -> Result<QuotePreview> {
        instructions::preview_quote::handler(ctx, notional_usdc)
    }

    pub fn accept_quote(ctx: Context<AcceptQuote>, args: AcceptQuoteArgs) -> Result<()> {
        instructions::accept_quote::handler(ctx, args)
    }

    pub fn record_observation(ctx: Context<RecordObservation>, expected_index: u8) -> Result<()> {
        instructions::record_observation::handler(ctx, expected_index)
    }

    pub fn settle(ctx: Context<Settle>) -> Result<()> {
        instructions::settle::handler(ctx)
    }

    pub fn write_reduced_operators(
        ctx: Context<WriteReducedOperators>,
        args: WriteReducedOperatorsArgs,
    ) -> Result<()> {
        instructions::write_reduced_operators::handler(ctx, args)
    }
}
