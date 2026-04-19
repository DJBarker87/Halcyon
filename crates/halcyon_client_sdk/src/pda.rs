use halcyon_common::seeds;
use solana_sdk::pubkey::Pubkey;

pub fn protocol_config() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::PROTOCOL_CONFIG], &halcyon_kernel::ID)
}

pub fn vault_state() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::VAULT_STATE], &halcyon_kernel::ID)
}

pub fn fee_ledger() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::FEE_LEDGER], &halcyon_kernel::ID)
}

pub fn keeper_registry() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::KEEPER_REGISTRY], &halcyon_kernel::ID)
}

pub fn vault_authority() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::VAULT_AUTHORITY], &halcyon_kernel::ID)
}

pub fn vault_usdc(usdc_mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::VAULT_USDC, usdc_mint.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn treasury_usdc(usdc_mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::TREASURY_USDC, usdc_mint.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn coupon_vault(product_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::COUPON_VAULT, product_program_id.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn coupon_vault_usdc(product_program_id: &Pubkey, usdc_mint: &Pubkey) -> Pubkey {
    let (coupon_vault, _) = coupon_vault(product_program_id);
    associated_token_account(&coupon_vault, usdc_mint)
}

pub fn hedge_sleeve(product_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::HEDGE_SLEEVE, product_program_id.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn hedge_sleeve_usdc(product_program_id: &Pubkey, usdc_mint: &Pubkey) -> Pubkey {
    let (hedge_sleeve, _) = hedge_sleeve(product_program_id);
    associated_token_account(&hedge_sleeve, usdc_mint)
}

pub fn hedge_sleeve_wsol(product_program_id: &Pubkey) -> Pubkey {
    let (hedge_sleeve, _) = hedge_sleeve(product_program_id);
    associated_token_account(
        &hedge_sleeve,
        &anchor_spl::token::spl_token::native_mint::ID,
    )
}

pub fn senior(owner: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::SENIOR, owner.as_ref()], &halcyon_kernel::ID)
}

pub fn junior(owner: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::JUNIOR, owner.as_ref()], &halcyon_kernel::ID)
}

pub fn product_registry_entry(product_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::PRODUCT_REGISTRY, product_program_id.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn vault_sigma(product_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::VAULT_SIGMA, product_program_id.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn regime_signal(product_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::REGIME_SIGNAL, product_program_id.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn hedge_book(product_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::HEDGE_BOOK, product_program_id.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn pending_hedge_swap(product_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::PENDING_HEDGE_SWAP, product_program_id.as_ref()],
        &halcyon_kernel::ID,
    )
}

pub fn policy(policy_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::POLICY, policy_id.as_ref()], &halcyon_kernel::ID)
}

pub fn terms(policy_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::TERMS, policy_id.as_ref()],
        &halcyon_sol_autocall::ID,
    )
}

pub fn product_authority() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::PRODUCT_AUTHORITY], &halcyon_sol_autocall::ID)
}

pub fn associated_token_account(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    spl_associated_token_account::get_associated_token_address(owner, mint)
}
