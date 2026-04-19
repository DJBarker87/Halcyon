use anchor_lang::prelude::*;
use halcyon_common::HalcyonError;
use solana_sha256_hasher::hashv;

use crate::{state::*, KernelError};

/// The second half of the mutual-CPI pattern.
///
/// `reserve_and_issue` creates `PolicyHeader` in `Quoted`. The product then
/// writes its `ProductTerms` account. `finalize_policy` re-borrows the header
/// cleanly, validates that the product_terms account the product wrote is
/// genuinely owned by the registered product program and hashes to the
/// `terms_hash` committed at issuance, then flips the header to `Active`.
///
/// The split is deliberate: per §3.5 of the L1 plan, Anchor's account-
/// constraints macro can re-lock accounts across a CPI boundary, so we cannot
/// mutate the header, CPI out, and mutate the header again in a single handler.
///
/// # Security — what this handler defends against
///
/// - **K1 (fictional terms binding).** Without an owner + discriminator
///   check, a registered product could pass `Pubkey::default()` / a
///   system-owned account / an account belonging to some other program and
///   the kernel would happily record the address as the policy's terms.
///   `ProductTerms` would then be unverifiable in any later review.
/// - **K2 (self-attested hash).** Without rehashing the bytes on-chain at
///   finalize time, a product could commit to one set of terms in
///   `reserve_and_issue.args.terms_hash` and write different bytes at the
///   terms address. The verifiability claim in `integration_architecture.md`
///   §2.13 depends on the rehash being done *inside the kernel*.
/// - **K7 (pause gate).** Admin `pause_issuance` must cleanly halt mid-flight
///   issuance, including policies that completed `reserve_and_issue` but have
///   not yet been finalized.
#[derive(Accounts)]
pub struct FinalizePolicy<'info> {
    /// Product authority PDA signing for its registered product.
    pub product_authority: Signer<'info>,

    // See LEARNED.md: seed constraints on kernel-owned PDAs validated inside
    // a product->kernel CPI trigger an Anchor/SBF aliasing bug on 0.32.1.
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,

    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(mut)]
    pub policy_header: Account<'info, PolicyHeader>,

    /// CHECK: the product-specific terms account. Validated below:
    /// owner == product_program_id, non-empty with a non-zero discriminator,
    /// and its bytes hash to `policy_header.terms_hash`.
    pub product_terms: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<FinalizePolicy>) -> Result<()> {
    // --- 1. Authentication ---
    require_keys_eq!(
        ctx.accounts.product_authority.key(),
        ctx.accounts.product_registry_entry.expected_authority,
        HalcyonError::ProductAuthorityMismatch
    );

    // --- 2. Product program id must match between header and registry ---
    require_keys_eq!(
        ctx.accounts.policy_header.product_program_id,
        ctx.accounts.product_registry_entry.product_program_id,
        KernelError::ProductProgramMismatch
    );

    // --- 3. Pause gates: global + per-product ---
    require!(
        !ctx.accounts.protocol_config.issuance_paused_global,
        HalcyonError::PausedGlobally
    );
    require!(
        !ctx.accounts.product_registry_entry.paused,
        HalcyonError::IssuancePausedPerProduct
    );

    // --- 4. Header must be in Quoted ---
    require!(
        ctx.accounts.policy_header.status == PolicyStatus::Quoted,
        HalcyonError::PolicyNotQuoted
    );

    // --- 5. Validate the product_terms account we're about to bind ---
    let terms_info = ctx.accounts.product_terms.to_account_info();

    // 5a. Owner must be the registered product program (not system, not any
    //     unrelated program, not the kernel itself).
    require_keys_eq!(
        *terms_info.owner,
        ctx.accounts.product_registry_entry.product_program_id,
        HalcyonError::TermsAccountInvalid
    );

    // 5b. Account must be non-empty. Anchor-initialised accounts carry at
    //     least an 8-byte discriminator.
    let data = terms_info.try_borrow_data()?;
    require!(data.len() >= 8, HalcyonError::TermsAccountInvalid);

    // 5c. Discriminator must be non-zero (Anchor-initialised accounts get a
    //     sha256("account:<Name>")[..8] stamp; rejecting all-zero catches
    //     "account exists as a rent-paying shell but product never wrote
    //     anything to it").
    let discriminator = &data[..8];
    require!(
        discriminator.iter().any(|b| *b != 0),
        HalcyonError::TermsAccountInvalid
    );
    require!(
        discriminator
            == ctx
                .accounts
                .product_registry_entry
                .init_terms_discriminator
                .as_ref(),
        HalcyonError::TermsAccountInvalid
    );

    // 5d. Bytes (including discriminator) must hash to the committed
    //     terms_hash the product bound at `reserve_and_issue` time.
    //     Hashing the full bytes — not only the payload — forecloses the
    //     collision class where a product swaps the discriminator for a
    //     different Anchor account type with an identical payload layout.
    let computed = hashv(&[&data]).to_bytes();
    drop(data);
    require!(
        computed == ctx.accounts.policy_header.terms_hash,
        HalcyonError::TermsHashMismatch
    );

    // --- 6. Flip to Active + record terms address ---
    let header = &mut ctx.accounts.policy_header;
    header.product_terms = ctx.accounts.product_terms.key();
    header.status = PolicyStatus::Active;
    Ok(())
}
