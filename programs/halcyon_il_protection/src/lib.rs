//! IL Protection — L3.
//!
//! Synthetic 30-day SOL/USDC IL cover:
//! - `preview_quote` — read-only premium / liability quote
//! - `accept_quote`  — bounded issuance path through the shared kernel
//! - `settle`        — expiry-only settlement against Pyth SOL/USD and USDC/USD

use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod pricing;
pub mod state;

pub use errors::IlProtectionError;
#[allow(ambiguous_glob_reexports)]
pub use instructions::accept_quote::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::preview_quote::*;
#[allow(ambiguous_glob_reexports)]
pub use instructions::settle::*;
pub use state::*;

declare_id!("HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g");

#[program]
pub mod halcyon_il_protection {
    use super::*;

    pub fn preview_quote(
        ctx: Context<PreviewQuote>,
        insured_notional_usdc: u64,
    ) -> Result<QuotePreview> {
        instructions::preview_quote::handler(ctx, insured_notional_usdc)
    }

    pub fn accept_quote(ctx: Context<AcceptQuote>, args: AcceptQuoteArgs) -> Result<()> {
        instructions::accept_quote::handler(ctx, args)
    }

    pub fn settle(ctx: Context<Settle>) -> Result<()> {
        instructions::settle::handler(ctx)
    }
}
