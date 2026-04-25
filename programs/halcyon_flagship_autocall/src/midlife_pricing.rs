use anchor_lang::prelude::*;
use halcyon_common::HalcyonError;
use halcyon_flagship_quote::midlife_pricer::{
    advance_midlife_nav_in_place, advance_midlife_nav_monthly_debug_in_place,
    checkpoint_next_coupon_index, compute_midlife_nav, finish_midlife_nav_from_bytes,
    finish_midlife_nav_monthly_debug_from_bytes, start_midlife_nav_into,
    start_midlife_nav_monthly_debug_into, MidlifeInputs, MidlifeNav as QuoteMidlifeNav,
    MidlifePricerError, MIDLIFE_CHECKPOINT_BYTES, MIDLIFE_CHECKPOINT_VERSION,
};
use halcyon_kernel::state::{PolicyHeader, PolicyStatus, ProtocolConfig, Regression, VaultSigma};

use crate::calendar::trading_days_elapsed_since_issue;
use crate::errors::FlagshipAutocallError;
use crate::pricing::{
    compose_pricing_sigma, cu_trace, protocol_sigma_floor_annualised_s6, require_regression_fresh,
    require_sigma_fresh,
};
use crate::state::{
    FlagshipAutocallTerms, MidlifeInputSnapshot, ProductStatus, MIDLIFE_NAV_CHECKPOINT_VERSION,
};

const MIDLIFE_NAV_CHECKPOINT_MAGIC: [u8; 8] = *b"MLNAVC01";
const PRICING_MONTHLY_COUPON_TRADING_DAY_BOUNDARIES: [u16; 18] = [
    21, 42, 63, 84, 105, 126, 147, 168, 189, 210, 231, 252, 273, 294, 315, 336, 357, 378,
];
const PRICING_QUARTERLY_AUTOCALL_TRADING_DAY_BOUNDARIES: [u16; 6] = [63, 126, 189, 252, 315, 378];
const ACCOUNT_MAGIC_OFFSET: usize = 0;
const ACCOUNT_VERSION_OFFSET: usize = ACCOUNT_MAGIC_OFFSET + 8;
const ACCOUNT_REQUESTER_OFFSET: usize = ACCOUNT_VERSION_OFFSET + 1;
const ACCOUNT_POLICY_HEADER_OFFSET: usize = ACCOUNT_REQUESTER_OFFSET + 32;
const ACCOUNT_PRODUCT_TERMS_OFFSET: usize = ACCOUNT_POLICY_HEADER_OFFSET + 32;
const ACCOUNT_PREPARED_SLOT_OFFSET: usize = ACCOUNT_PRODUCT_TERMS_OFFSET + 32;
const ACCOUNT_EXPIRES_AT_SLOT_OFFSET: usize = ACCOUNT_PREPARED_SLOT_OFFSET + 8;
const ACCOUNT_INPUTS_OFFSET: usize = ACCOUNT_EXPIRES_AT_SLOT_OFFSET + 8;
const ACCOUNT_CHECKPOINT_OFFSET: usize = ACCOUNT_INPUTS_OFFSET + MidlifeInputSnapshot::INIT_SPACE;
pub const MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE: usize =
    ACCOUNT_CHECKPOINT_OFFSET + MIDLIFE_CHECKPOINT_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidlifeValuation {
    pub nav: QuoteMidlifeNav,
    pub sigma_pricing_s6: i64,
    pub now_trading_day: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedMidlifeNav {
    pub inputs: MidlifeInputs,
    pub next_coupon_index: u8,
    pub final_coupon_index: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidlifeCheckpointAccountView {
    pub inputs: MidlifeInputs,
    pub next_coupon_index: u8,
    pub final_coupon_index: u8,
    pub prepared_slot: u64,
    pub expires_at_slot: u64,
}

fn map_midlife_pricer_error(err: MidlifePricerError) -> Error {
    match err {
        MidlifePricerError::NotImplemented => error!(FlagshipAutocallError::MidlifeNavUnavailable),
        MidlifePricerError::InvalidInput => error!(FlagshipAutocallError::MidlifeNavInvalid),
        MidlifePricerError::MathError => error!(FlagshipAutocallError::MidlifeNavMathFailed),
    }
}

fn trading_day_schedule<const N: usize>(boundaries: &[u16; N]) -> [i64; N] {
    let mut out = [0i64; N];
    let mut idx = 0;
    while idx < N {
        out[idx] = i64::from(boundaries[idx]);
        idx += 1;
    }
    out
}

#[inline(always)]
fn pricing_trading_day(elapsed_trading_day: u16) -> u16 {
    #[cfg(feature = "integration-test")]
    {
        elapsed_trading_day.saturating_mul(21).min(378)
    }
    #[cfg(not(feature = "integration-test"))]
    {
        elapsed_trading_day
    }
}

pub fn build_midlife_inputs(
    policy_header: &PolicyHeader,
    terms: &FlagshipAutocallTerms,
    regression: &Regression,
    live_spots_s6: [i64; 3],
    sigma_pricing_s6: i64,
    now_trading_day: u16,
) -> MidlifeInputs {
    let now_trading_day = pricing_trading_day(now_trading_day);
    MidlifeInputs {
        current_spy_s6: live_spots_s6[0],
        current_qqq_s6: live_spots_s6[1],
        current_iwm_s6: live_spots_s6[2],
        sigma_common_s6: sigma_pricing_s6,
        entry_spy_s6: terms.entry_spy_price_s6,
        entry_qqq_s6: terms.entry_qqq_price_s6,
        entry_iwm_s6: terms.entry_iwm_price_s6,
        beta_spy_s12: regression.beta_spy_s12,
        beta_qqq_s12: regression.beta_qqq_s12,
        alpha_s12: regression.alpha_s12,
        regression_residual_vol_s6: regression.residual_vol_s6,
        monthly_coupon_schedule: trading_day_schedule(
            &PRICING_MONTHLY_COUPON_TRADING_DAY_BOUNDARIES,
        ),
        quarterly_autocall_schedule: trading_day_schedule(
            &PRICING_QUARTERLY_AUTOCALL_TRADING_DAY_BOUNDARIES,
        ),
        next_coupon_index: terms.next_coupon_index,
        next_autocall_index: terms.next_autocall_index,
        offered_coupon_bps_s6: terms.offered_coupon_bps_s6,
        coupon_barrier_bps: terms.coupon_barrier_bps,
        autocall_barrier_bps: terms.autocall_barrier_bps,
        ki_barrier_bps: terms.ki_barrier_bps,
        ki_latched: terms.ki_latched,
        missed_coupon_observations: terms.missed_coupon_observations,
        coupons_paid_usdc: terms.coupons_paid_usdc,
        notional_usdc: policy_header.notional,
        now_trading_day,
    }
}

pub fn snapshot_from_inputs(inputs: &MidlifeInputs) -> MidlifeInputSnapshot {
    MidlifeInputSnapshot {
        current_spy_s6: inputs.current_spy_s6,
        current_qqq_s6: inputs.current_qqq_s6,
        current_iwm_s6: inputs.current_iwm_s6,
        sigma_common_s6: inputs.sigma_common_s6,
        entry_spy_s6: inputs.entry_spy_s6,
        entry_qqq_s6: inputs.entry_qqq_s6,
        entry_iwm_s6: inputs.entry_iwm_s6,
        beta_spy_s12: inputs.beta_spy_s12,
        beta_qqq_s12: inputs.beta_qqq_s12,
        alpha_s12: inputs.alpha_s12,
        regression_residual_vol_s6: inputs.regression_residual_vol_s6,
        monthly_coupon_schedule: inputs.monthly_coupon_schedule,
        quarterly_autocall_schedule: inputs.quarterly_autocall_schedule,
        next_coupon_index: inputs.next_coupon_index,
        next_autocall_index: inputs.next_autocall_index,
        offered_coupon_bps_s6: inputs.offered_coupon_bps_s6,
        coupon_barrier_bps: inputs.coupon_barrier_bps,
        autocall_barrier_bps: inputs.autocall_barrier_bps,
        ki_barrier_bps: inputs.ki_barrier_bps,
        ki_latched: inputs.ki_latched,
        missed_coupon_observations: inputs.missed_coupon_observations,
        coupons_paid_usdc: inputs.coupons_paid_usdc,
        notional_usdc: inputs.notional_usdc,
        now_trading_day: inputs.now_trading_day,
    }
}

pub fn inputs_from_snapshot(snapshot: &MidlifeInputSnapshot) -> MidlifeInputs {
    MidlifeInputs {
        current_spy_s6: snapshot.current_spy_s6,
        current_qqq_s6: snapshot.current_qqq_s6,
        current_iwm_s6: snapshot.current_iwm_s6,
        sigma_common_s6: snapshot.sigma_common_s6,
        entry_spy_s6: snapshot.entry_spy_s6,
        entry_qqq_s6: snapshot.entry_qqq_s6,
        entry_iwm_s6: snapshot.entry_iwm_s6,
        beta_spy_s12: snapshot.beta_spy_s12,
        beta_qqq_s12: snapshot.beta_qqq_s12,
        alpha_s12: snapshot.alpha_s12,
        regression_residual_vol_s6: snapshot.regression_residual_vol_s6,
        monthly_coupon_schedule: snapshot.monthly_coupon_schedule,
        quarterly_autocall_schedule: snapshot.quarterly_autocall_schedule,
        next_coupon_index: snapshot.next_coupon_index,
        next_autocall_index: snapshot.next_autocall_index,
        offered_coupon_bps_s6: snapshot.offered_coupon_bps_s6,
        coupon_barrier_bps: snapshot.coupon_barrier_bps,
        autocall_barrier_bps: snapshot.autocall_barrier_bps,
        ki_barrier_bps: snapshot.ki_barrier_bps,
        ki_latched: snapshot.ki_latched,
        missed_coupon_observations: snapshot.missed_coupon_observations,
        coupons_paid_usdc: snapshot.coupons_paid_usdc,
        notional_usdc: snapshot.notional_usdc,
        now_trading_day: snapshot.now_trading_day,
    }
}

#[inline(always)]
fn require_checkpoint_account_shape(checkpoint: &AccountInfo<'_>) -> Result<()> {
    require_keys_eq!(
        *checkpoint.owner,
        crate::ID,
        FlagshipAutocallError::MidlifeNavInvalid
    );
    require!(
        checkpoint.data_len() == MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE,
        FlagshipAutocallError::MidlifeNavInvalid
    );
    Ok(())
}

#[inline(always)]
fn read_u8(data: &[u8], offset: usize) -> Result<u8> {
    data.get(offset)
        .copied()
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))
}

#[inline(always)]
fn write_u8(data: &mut [u8], offset: usize, value: u8) {
    data[offset] = value;
}

#[inline(always)]
fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    let raw = data
        .get(offset..end)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    Ok(u16::from_le_bytes(raw.try_into().map_err(|_| {
        error!(FlagshipAutocallError::MidlifeNavInvalid)
    })?))
}

#[inline(always)]
fn write_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

#[inline(always)]
fn read_u64(data: &[u8], offset: usize) -> Result<u64> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    let raw = data
        .get(offset..end)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    Ok(u64::from_le_bytes(raw.try_into().map_err(|_| {
        error!(FlagshipAutocallError::MidlifeNavInvalid)
    })?))
}

#[inline(always)]
fn write_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[inline(always)]
fn read_i64(data: &[u8], offset: usize) -> Result<i64> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    let raw = data
        .get(offset..end)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    Ok(i64::from_le_bytes(raw.try_into().map_err(|_| {
        error!(FlagshipAutocallError::MidlifeNavInvalid)
    })?))
}

#[inline(always)]
fn write_i64(data: &mut [u8], offset: usize, value: i64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[inline(always)]
fn read_i128(data: &[u8], offset: usize) -> Result<i128> {
    let end = offset
        .checked_add(16)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    let raw = data
        .get(offset..end)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    Ok(i128::from_le_bytes(raw.try_into().map_err(|_| {
        error!(FlagshipAutocallError::MidlifeNavInvalid)
    })?))
}

#[inline(always)]
fn write_i128(data: &mut [u8], offset: usize, value: i128) {
    data[offset..offset + 16].copy_from_slice(&value.to_le_bytes());
}

#[inline(always)]
fn read_pubkey(data: &[u8], offset: usize) -> Result<Pubkey> {
    let end = offset
        .checked_add(32)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    let raw = data
        .get(offset..end)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    Ok(Pubkey::new_from_array(raw.try_into().map_err(|_| {
        error!(FlagshipAutocallError::MidlifeNavInvalid)
    })?))
}

#[inline(always)]
fn write_pubkey(data: &mut [u8], offset: usize, value: Pubkey) {
    data[offset..offset + 32].copy_from_slice(value.as_ref());
}

fn write_input_snapshot_bytes(data: &mut [u8], inputs: &MidlifeInputs) -> Result<()> {
    require!(
        data.len() == MidlifeInputSnapshot::INIT_SPACE,
        FlagshipAutocallError::MidlifeNavInvalid
    );
    let mut offset = 0usize;
    write_i64(data, offset, inputs.current_spy_s6);
    offset += 8;
    write_i64(data, offset, inputs.current_qqq_s6);
    offset += 8;
    write_i64(data, offset, inputs.current_iwm_s6);
    offset += 8;
    write_i64(data, offset, inputs.sigma_common_s6);
    offset += 8;
    write_i64(data, offset, inputs.entry_spy_s6);
    offset += 8;
    write_i64(data, offset, inputs.entry_qqq_s6);
    offset += 8;
    write_i64(data, offset, inputs.entry_iwm_s6);
    offset += 8;
    write_i128(data, offset, inputs.beta_spy_s12);
    offset += 16;
    write_i128(data, offset, inputs.beta_qqq_s12);
    offset += 16;
    write_i128(data, offset, inputs.alpha_s12);
    offset += 16;
    write_i64(data, offset, inputs.regression_residual_vol_s6);
    offset += 8;
    for value in inputs.monthly_coupon_schedule {
        write_i64(data, offset, value);
        offset += 8;
    }
    for value in inputs.quarterly_autocall_schedule {
        write_i64(data, offset, value);
        offset += 8;
    }
    write_u8(data, offset, inputs.next_coupon_index);
    offset += 1;
    write_u8(data, offset, inputs.next_autocall_index);
    offset += 1;
    write_i64(data, offset, inputs.offered_coupon_bps_s6);
    offset += 8;
    write_u16(data, offset, inputs.coupon_barrier_bps);
    offset += 2;
    write_u16(data, offset, inputs.autocall_barrier_bps);
    offset += 2;
    write_u16(data, offset, inputs.ki_barrier_bps);
    offset += 2;
    write_u8(data, offset, u8::from(inputs.ki_latched));
    offset += 1;
    write_u8(data, offset, inputs.missed_coupon_observations);
    offset += 1;
    write_u64(data, offset, inputs.coupons_paid_usdc);
    offset += 8;
    write_u64(data, offset, inputs.notional_usdc);
    offset += 8;
    write_u16(data, offset, inputs.now_trading_day);
    Ok(())
}

fn read_input_snapshot_bytes(data: &[u8]) -> Result<MidlifeInputs> {
    require!(
        data.len() == MidlifeInputSnapshot::INIT_SPACE,
        FlagshipAutocallError::MidlifeNavInvalid
    );
    let mut offset = 0usize;
    let current_spy_s6 = read_i64(data, offset)?;
    offset += 8;
    let current_qqq_s6 = read_i64(data, offset)?;
    offset += 8;
    let current_iwm_s6 = read_i64(data, offset)?;
    offset += 8;
    let sigma_common_s6 = read_i64(data, offset)?;
    offset += 8;
    let entry_spy_s6 = read_i64(data, offset)?;
    offset += 8;
    let entry_qqq_s6 = read_i64(data, offset)?;
    offset += 8;
    let entry_iwm_s6 = read_i64(data, offset)?;
    offset += 8;
    let beta_spy_s12 = read_i128(data, offset)?;
    offset += 16;
    let beta_qqq_s12 = read_i128(data, offset)?;
    offset += 16;
    let alpha_s12 = read_i128(data, offset)?;
    offset += 16;
    let regression_residual_vol_s6 = read_i64(data, offset)?;
    offset += 8;
    let mut monthly_coupon_schedule = [0i64; 18];
    let mut idx = 0usize;
    while idx < monthly_coupon_schedule.len() {
        monthly_coupon_schedule[idx] = read_i64(data, offset)?;
        offset += 8;
        idx += 1;
    }
    let mut quarterly_autocall_schedule = [0i64; 6];
    idx = 0;
    while idx < quarterly_autocall_schedule.len() {
        quarterly_autocall_schedule[idx] = read_i64(data, offset)?;
        offset += 8;
        idx += 1;
    }
    let next_coupon_index = read_u8(data, offset)?;
    offset += 1;
    let next_autocall_index = read_u8(data, offset)?;
    offset += 1;
    let offered_coupon_bps_s6 = read_i64(data, offset)?;
    offset += 8;
    let coupon_barrier_bps = read_u16(data, offset)?;
    offset += 2;
    let autocall_barrier_bps = read_u16(data, offset)?;
    offset += 2;
    let ki_barrier_bps = read_u16(data, offset)?;
    offset += 2;
    let ki_latched_raw = read_u8(data, offset)?;
    offset += 1;
    require!(
        ki_latched_raw <= 1,
        FlagshipAutocallError::MidlifeNavInvalid
    );
    let missed_coupon_observations = read_u8(data, offset)?;
    offset += 1;
    let coupons_paid_usdc = read_u64(data, offset)?;
    offset += 8;
    let notional_usdc = read_u64(data, offset)?;
    offset += 8;
    let now_trading_day = read_u16(data, offset)?;
    Ok(MidlifeInputs {
        current_spy_s6,
        current_qqq_s6,
        current_iwm_s6,
        sigma_common_s6,
        entry_spy_s6,
        entry_qqq_s6,
        entry_iwm_s6,
        beta_spy_s12,
        beta_qqq_s12,
        alpha_s12,
        regression_residual_vol_s6,
        monthly_coupon_schedule,
        quarterly_autocall_schedule,
        next_coupon_index,
        next_autocall_index,
        offered_coupon_bps_s6,
        coupon_barrier_bps,
        autocall_barrier_bps,
        ki_barrier_bps,
        ki_latched: ki_latched_raw == 1,
        missed_coupon_observations,
        coupons_paid_usdc,
        notional_usdc,
        now_trading_day,
    })
}

fn checkpoint_account_inputs(data: &[u8]) -> Result<MidlifeInputs> {
    read_input_snapshot_bytes(
        data.get(ACCOUNT_INPUTS_OFFSET..ACCOUNT_CHECKPOINT_OFFSET)
            .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?,
    )
}

fn checkpoint_bytes(data: &[u8]) -> Result<&[u8]> {
    data.get(ACCOUNT_CHECKPOINT_OFFSET..MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))
}

fn checkpoint_bytes_mut(data: &mut [u8]) -> Result<&mut [u8]> {
    data.get_mut(ACCOUNT_CHECKPOINT_OFFSET..MIDLIFE_NAV_CHECKPOINT_ACCOUNT_SPACE)
        .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))
}

pub fn default_midlife_stop_coupon_index(inputs: &MidlifeInputs) -> Result<u8> {
    let start = inputs.next_coupon_index as usize;
    let end = inputs.monthly_coupon_schedule.len();
    require!(start < end, FlagshipAutocallError::MidlifeNavInvalid);
    let remaining = end - start;
    let stop = start + (remaining / 2).max(1);
    u8::try_from(stop).map_err(|_| error!(FlagshipAutocallError::MidlifeNavInvalid))
}

pub fn validate_checkpoint_account(
    checkpoint: &AccountInfo<'_>,
    requester: Pubkey,
    policy_header_key: Pubkey,
    product_terms_key: Pubkey,
    current_slot: u64,
) -> Result<()> {
    require_checkpoint_account_shape(checkpoint)?;
    let data = checkpoint.try_borrow_data()?;
    require!(
        data.get(ACCOUNT_MAGIC_OFFSET..ACCOUNT_MAGIC_OFFSET + 8)
            == Some(MIDLIFE_NAV_CHECKPOINT_MAGIC.as_slice()),
        FlagshipAutocallError::MidlifeNavInvalid
    );
    require!(
        read_u8(&data, ACCOUNT_VERSION_OFFSET)? == MIDLIFE_NAV_CHECKPOINT_VERSION,
        FlagshipAutocallError::MidlifeNavInvalid
    );
    require_keys_eq!(
        read_pubkey(&data, ACCOUNT_REQUESTER_OFFSET)?,
        requester,
        FlagshipAutocallError::PolicyStateInvalid
    );
    require_keys_eq!(
        read_pubkey(&data, ACCOUNT_POLICY_HEADER_OFFSET)?,
        policy_header_key,
        FlagshipAutocallError::PolicyStateInvalid
    );
    require_keys_eq!(
        read_pubkey(&data, ACCOUNT_PRODUCT_TERMS_OFFSET)?,
        product_terms_key,
        FlagshipAutocallError::PolicyStateInvalid
    );
    require!(
        current_slot <= read_u64(&data, ACCOUNT_EXPIRES_AT_SLOT_OFFSET)?,
        FlagshipAutocallError::MidlifeNavUnavailable
    );
    Ok(())
}

pub fn require_checkpoint_matches_policy_state(
    checkpoint: &AccountInfo<'_>,
    policy_header: &PolicyHeader,
    terms: &FlagshipAutocallTerms,
) -> Result<()> {
    let data = checkpoint.try_borrow_data()?;
    let inputs = checkpoint_account_inputs(&data)?;
    require!(
        policy_header.notional == inputs.notional_usdc
            && policy_header.status == PolicyStatus::Active
            && terms.status == ProductStatus::Active
            && terms.entry_spy_price_s6 == inputs.entry_spy_s6
            && terms.entry_qqq_price_s6 == inputs.entry_qqq_s6
            && terms.entry_iwm_price_s6 == inputs.entry_iwm_s6
            && terms.next_coupon_index == inputs.next_coupon_index
            && terms.next_autocall_index == inputs.next_autocall_index
            && terms.offered_coupon_bps_s6 == inputs.offered_coupon_bps_s6
            && terms.coupon_barrier_bps == inputs.coupon_barrier_bps
            && terms.autocall_barrier_bps == inputs.autocall_barrier_bps
            && terms.ki_barrier_bps == inputs.ki_barrier_bps
            && terms.ki_latched == inputs.ki_latched
            && terms.missed_coupon_observations == inputs.missed_coupon_observations
            && terms.coupons_paid_usdc == inputs.coupons_paid_usdc,
        FlagshipAutocallError::PolicyStateInvalid
    );
    Ok(())
}

pub fn read_live_spots_from_accounts<'info>(
    protocol_config: &ProtocolConfig,
    pyth_spy: &AccountInfo<'info>,
    pyth_qqq: &AccountInfo<'info>,
    pyth_iwm: &AccountInfo<'info>,
    clock: &Clock,
) -> Result<[i64; 3]> {
    let spy = halcyon_oracles::read_pyth_price(
        pyth_spy,
        &halcyon_oracles::feed_ids::SPY_USD,
        &crate::ID,
        clock,
        protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let qqq = halcyon_oracles::read_pyth_price(
        pyth_qqq,
        &halcyon_oracles::feed_ids::QQQ_USD,
        &crate::ID,
        clock,
        protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    let iwm = halcyon_oracles::read_pyth_price(
        pyth_iwm,
        &halcyon_oracles::feed_ids::IWM_USD,
        &crate::ID,
        clock,
        protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    Ok([spy.price_s6, qqq.price_s6, iwm.price_s6])
}

pub fn compute_nav_from_market_state(
    policy_header: &PolicyHeader,
    terms: &FlagshipAutocallTerms,
    regression: &Regression,
    live_spots_s6: [i64; 3],
    sigma_pricing_s6: i64,
    now_trading_day: u16,
) -> Result<MidlifeValuation> {
    require!(
        policy_header.status == PolicyStatus::Active && terms.status == ProductStatus::Active,
        FlagshipAutocallError::MidlifeNavUnavailable
    );

    let inputs = build_midlife_inputs(
        policy_header,
        terms,
        regression,
        live_spots_s6,
        sigma_pricing_s6,
        now_trading_day,
    );
    let nav = compute_midlife_nav(&inputs).map_err(map_midlife_pricer_error)?;

    Ok(MidlifeValuation {
        nav,
        sigma_pricing_s6,
        now_trading_day,
    })
}

pub fn compute_nav_from_accounts<'info>(
    protocol_config: &ProtocolConfig,
    vault_sigma: &VaultSigma,
    regression: &Regression,
    policy_header: &PolicyHeader,
    terms: &FlagshipAutocallTerms,
    pyth_spy: &AccountInfo<'info>,
    pyth_qqq: &AccountInfo<'info>,
    pyth_iwm: &AccountInfo<'info>,
    clock: &Clock,
) -> Result<MidlifeValuation> {
    let now = clock.unix_timestamp;
    require_sigma_fresh(vault_sigma, now, protocol_config.sigma_staleness_cap_secs)?;
    require_regression_fresh(
        regression,
        now,
        protocol_config.regression_staleness_cap_secs,
    )?;
    cu_trace("flagship midlife after freshness checks");

    let live_spots_s6 =
        read_live_spots_from_accounts(protocol_config, pyth_spy, pyth_qqq, pyth_iwm, clock)?;
    cu_trace("flagship midlife after oracle reads");

    let sigma_pricing_s6 = compose_pricing_sigma(
        vault_sigma,
        protocol_sigma_floor_annualised_s6(protocol_config),
        protocol_config.sigma_ceiling_annualised_s6,
    )?;
    let now_trading_day = trading_days_elapsed_since_issue(policy_header.issued_at, now)?;
    cu_trace("flagship midlife after sigma/time compose");

    compute_nav_from_market_state(
        policy_header,
        terms,
        regression,
        live_spots_s6,
        sigma_pricing_s6,
        now_trading_day,
    )
}

pub fn prepare_nav_from_accounts<'info>(
    protocol_config: &ProtocolConfig,
    vault_sigma: &VaultSigma,
    regression: &Regression,
    policy_header: &PolicyHeader,
    terms: &FlagshipAutocallTerms,
    pyth_spy: &AccountInfo<'info>,
    pyth_qqq: &AccountInfo<'info>,
    pyth_iwm: &AccountInfo<'info>,
    clock: &Clock,
    stop_coupon_index: u8,
) -> Result<PreparedMidlifeNav> {
    require!(
        policy_header.status == PolicyStatus::Active && terms.status == ProductStatus::Active,
        FlagshipAutocallError::MidlifeNavUnavailable
    );

    let now = clock.unix_timestamp;
    require_sigma_fresh(vault_sigma, now, protocol_config.sigma_staleness_cap_secs)?;
    require_regression_fresh(
        regression,
        now,
        protocol_config.regression_staleness_cap_secs,
    )?;
    cu_trace("flagship midlife prepare after freshness checks");

    let live_spots_s6 =
        read_live_spots_from_accounts(protocol_config, pyth_spy, pyth_qqq, pyth_iwm, clock)?;
    cu_trace("flagship midlife prepare after oracle reads");

    let sigma_pricing_s6 = compose_pricing_sigma(
        vault_sigma,
        protocol_sigma_floor_annualised_s6(protocol_config),
        protocol_config.sigma_ceiling_annualised_s6,
    )?;
    let now_trading_day = trading_days_elapsed_since_issue(policy_header.issued_at, now)?;
    cu_trace("flagship midlife prepare after sigma/time compose");

    let inputs = build_midlife_inputs(
        policy_header,
        terms,
        regression,
        live_spots_s6,
        sigma_pricing_s6,
        now_trading_day,
    );
    let final_coupon_index = u8::try_from(inputs.monthly_coupon_schedule.len())
        .map_err(|_| error!(FlagshipAutocallError::MidlifeNavInvalid))?;
    require!(
        stop_coupon_index >= inputs.next_coupon_index && stop_coupon_index <= final_coupon_index,
        FlagshipAutocallError::MidlifeNavInvalid
    );

    Ok(PreparedMidlifeNav {
        inputs,
        next_coupon_index: stop_coupon_index,
        final_coupon_index,
    })
}

pub fn write_checkpoint_account_from_inputs(
    checkpoint_account: &AccountInfo<'_>,
    requester: Pubkey,
    policy_header_key: Pubkey,
    product_terms_key: Pubkey,
    prepared_slot: u64,
    expires_at_slot: u64,
    inputs: &MidlifeInputs,
    stop_coupon_index: u8,
) -> Result<MidlifeCheckpointAccountView> {
    require!(
        MIDLIFE_NAV_CHECKPOINT_VERSION == MIDLIFE_CHECKPOINT_VERSION,
        FlagshipAutocallError::MidlifeNavInvalid
    );
    require_checkpoint_account_shape(checkpoint_account)?;
    let mut data = checkpoint_account.try_borrow_mut_data()?;
    require!(
        data.get(ACCOUNT_MAGIC_OFFSET..ACCOUNT_MAGIC_OFFSET + 8) == Some(&[0u8; 8][..]),
        FlagshipAutocallError::MidlifeNavInvalid
    );

    {
        let checkpoint = checkpoint_bytes_mut(&mut data)?;
        start_midlife_nav_into(inputs, stop_coupon_index, checkpoint)
            .map_err(map_midlife_pricer_error)?;
    }

    data[ACCOUNT_MAGIC_OFFSET..ACCOUNT_MAGIC_OFFSET + 8]
        .copy_from_slice(&MIDLIFE_NAV_CHECKPOINT_MAGIC);
    write_u8(
        &mut data,
        ACCOUNT_VERSION_OFFSET,
        MIDLIFE_NAV_CHECKPOINT_VERSION,
    );
    write_pubkey(&mut data, ACCOUNT_REQUESTER_OFFSET, requester);
    write_pubkey(&mut data, ACCOUNT_POLICY_HEADER_OFFSET, policy_header_key);
    write_pubkey(&mut data, ACCOUNT_PRODUCT_TERMS_OFFSET, product_terms_key);
    write_u64(&mut data, ACCOUNT_PREPARED_SLOT_OFFSET, prepared_slot);
    write_u64(&mut data, ACCOUNT_EXPIRES_AT_SLOT_OFFSET, expires_at_slot);
    write_input_snapshot_bytes(
        data.get_mut(ACCOUNT_INPUTS_OFFSET..ACCOUNT_CHECKPOINT_OFFSET)
            .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?,
        inputs,
    )?;

    checkpoint_account_view_from_data(&data)
}

#[cfg(any(feature = "integration-test", feature = "idl-build"))]
pub fn write_monthly_debug_checkpoint_account_from_inputs(
    checkpoint_account: &AccountInfo<'_>,
    requester: Pubkey,
    policy_header_key: Pubkey,
    product_terms_key: Pubkey,
    prepared_slot: u64,
    expires_at_slot: u64,
    inputs: &MidlifeInputs,
    stop_coupon_index: u8,
) -> Result<MidlifeCheckpointAccountView> {
    require!(
        MIDLIFE_NAV_CHECKPOINT_VERSION == MIDLIFE_CHECKPOINT_VERSION,
        FlagshipAutocallError::MidlifeNavInvalid
    );
    require_checkpoint_account_shape(checkpoint_account)?;
    let mut data = checkpoint_account.try_borrow_mut_data()?;
    require!(
        data.get(ACCOUNT_MAGIC_OFFSET..ACCOUNT_MAGIC_OFFSET + 8) == Some(&[0u8; 8][..]),
        FlagshipAutocallError::MidlifeNavInvalid
    );

    {
        let checkpoint = checkpoint_bytes_mut(&mut data)?;
        start_midlife_nav_monthly_debug_into(inputs, stop_coupon_index, checkpoint)
            .map_err(map_midlife_pricer_error)?;
    }

    data[ACCOUNT_MAGIC_OFFSET..ACCOUNT_MAGIC_OFFSET + 8]
        .copy_from_slice(&MIDLIFE_NAV_CHECKPOINT_MAGIC);
    write_u8(
        &mut data,
        ACCOUNT_VERSION_OFFSET,
        MIDLIFE_NAV_CHECKPOINT_VERSION,
    );
    write_pubkey(&mut data, ACCOUNT_REQUESTER_OFFSET, requester);
    write_pubkey(&mut data, ACCOUNT_POLICY_HEADER_OFFSET, policy_header_key);
    write_pubkey(&mut data, ACCOUNT_PRODUCT_TERMS_OFFSET, product_terms_key);
    write_u64(&mut data, ACCOUNT_PREPARED_SLOT_OFFSET, prepared_slot);
    write_u64(&mut data, ACCOUNT_EXPIRES_AT_SLOT_OFFSET, expires_at_slot);
    write_input_snapshot_bytes(
        data.get_mut(ACCOUNT_INPUTS_OFFSET..ACCOUNT_CHECKPOINT_OFFSET)
            .ok_or_else(|| error!(FlagshipAutocallError::MidlifeNavInvalid))?,
        inputs,
    )?;

    checkpoint_account_view_from_data(&data)
}

fn checkpoint_account_view_from_data(data: &[u8]) -> Result<MidlifeCheckpointAccountView> {
    let inputs = checkpoint_account_inputs(data)?;
    let next_coupon_index =
        checkpoint_next_coupon_index(checkpoint_bytes(data)?).map_err(map_midlife_pricer_error)?;
    Ok(MidlifeCheckpointAccountView {
        final_coupon_index: inputs.monthly_coupon_schedule.len() as u8,
        inputs,
        next_coupon_index,
        prepared_slot: read_u64(data, ACCOUNT_PREPARED_SLOT_OFFSET)?,
        expires_at_slot: read_u64(data, ACCOUNT_EXPIRES_AT_SLOT_OFFSET)?,
    })
}

pub fn checkpoint_account_view(
    checkpoint_account: &AccountInfo<'_>,
) -> Result<MidlifeCheckpointAccountView> {
    require_checkpoint_account_shape(checkpoint_account)?;
    let data = checkpoint_account.try_borrow_data()?;
    checkpoint_account_view_from_data(&data)
}

pub fn finish_nav_from_checkpoint(
    checkpoint_account: &AccountInfo<'_>,
) -> Result<MidlifeValuation> {
    require_checkpoint_account_shape(checkpoint_account)?;
    let data = checkpoint_account.try_borrow_data()?;
    let inputs = checkpoint_account_inputs(&data)?;
    let nav = finish_midlife_nav_from_bytes(&inputs, checkpoint_bytes(&data)?)
        .map_err(map_midlife_pricer_error)?;
    Ok(MidlifeValuation {
        nav,
        sigma_pricing_s6: inputs.sigma_common_s6,
        now_trading_day: inputs.now_trading_day,
    })
}

#[cfg(any(feature = "integration-test", feature = "idl-build"))]
pub fn finish_monthly_debug_nav_from_checkpoint(
    checkpoint_account: &AccountInfo<'_>,
) -> Result<MidlifeValuation> {
    require_checkpoint_account_shape(checkpoint_account)?;
    let data = checkpoint_account.try_borrow_data()?;
    let inputs = checkpoint_account_inputs(&data)?;
    let nav = finish_midlife_nav_monthly_debug_from_bytes(&inputs, checkpoint_bytes(&data)?)
        .map_err(map_midlife_pricer_error)?;
    Ok(MidlifeValuation {
        nav,
        sigma_pricing_s6: inputs.sigma_common_s6,
        now_trading_day: inputs.now_trading_day,
    })
}

pub fn advance_checkpoint(
    checkpoint_account: &AccountInfo<'_>,
    stop_coupon_index: u8,
) -> Result<MidlifeCheckpointAccountView> {
    require_checkpoint_account_shape(checkpoint_account)?;
    let mut data = checkpoint_account.try_borrow_mut_data()?;
    let inputs = checkpoint_account_inputs(&data)?;
    let start =
        checkpoint_next_coupon_index(checkpoint_bytes(&data)?).map_err(map_midlife_pricer_error)?;
    require!(
        stop_coupon_index > start
            && stop_coupon_index as usize <= inputs.monthly_coupon_schedule.len(),
        FlagshipAutocallError::MidlifeNavInvalid
    );
    advance_midlife_nav_in_place(&inputs, checkpoint_bytes_mut(&mut data)?, stop_coupon_index)
        .map_err(map_midlife_pricer_error)?;
    checkpoint_account_view_from_data(&data)
}

#[cfg(any(feature = "integration-test", feature = "idl-build"))]
pub fn advance_monthly_debug_checkpoint(
    checkpoint_account: &AccountInfo<'_>,
    stop_coupon_index: u8,
) -> Result<MidlifeCheckpointAccountView> {
    require_checkpoint_account_shape(checkpoint_account)?;
    let mut data = checkpoint_account.try_borrow_mut_data()?;
    let inputs = checkpoint_account_inputs(&data)?;
    let start =
        checkpoint_next_coupon_index(checkpoint_bytes(&data)?).map_err(map_midlife_pricer_error)?;
    require!(
        stop_coupon_index > start
            && stop_coupon_index as usize <= inputs.monthly_coupon_schedule.len(),
        FlagshipAutocallError::MidlifeNavInvalid
    );
    advance_midlife_nav_monthly_debug_in_place(
        &inputs,
        checkpoint_bytes_mut(&mut data)?,
        stop_coupon_index,
    )
    .map_err(map_midlife_pricer_error)?;
    checkpoint_account_view_from_data(&data)
}

pub fn close_checkpoint_account(
    checkpoint_account: &AccountInfo<'_>,
    recipient: &AccountInfo<'_>,
) -> Result<()> {
    require_checkpoint_account_shape(checkpoint_account)?;
    let lamports = checkpoint_account.lamports();
    **recipient.try_borrow_mut_lamports()? = recipient
        .lamports()
        .checked_add(lamports)
        .ok_or(HalcyonError::Overflow)?;
    **checkpoint_account.try_borrow_mut_lamports()? = 0;
    checkpoint_account.try_borrow_mut_data()?.fill(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AUTOCALL_BARRIER_BPS, COUPON_BARRIER_BPS, KI_BARRIER_BPS};
    use crate::state::{MONTHLY_COUPON_COUNT, QUARTERLY_AUTOCALL_COUNT};

    fn sample_policy_header() -> PolicyHeader {
        PolicyHeader {
            version: PolicyHeader::CURRENT_VERSION,
            product_program_id: crate::ID,
            owner: Pubkey::new_unique(),
            notional: 100_000_000,
            premium_paid: 0,
            max_liability: 100_000_000,
            issued_at: 0,
            expiry_ts: 0,
            quote_expiry_ts: 0,
            settled_at: 0,
            terms_hash: [0; 32],
            engine_version: 1,
            status: PolicyStatus::Active,
            product_terms: Pubkey::new_unique(),
            shard_id: 0,
            policy_id: Pubkey::new_unique(),
        }
    }

    fn sample_terms() -> FlagshipAutocallTerms {
        FlagshipAutocallTerms {
            version: FlagshipAutocallTerms::CURRENT_VERSION,
            policy_header: Pubkey::new_unique(),
            entry_spy_price_s6: 100_000_000,
            entry_qqq_price_s6: 100_000_000,
            entry_iwm_price_s6: 100_000_000,
            monthly_coupon_schedule: [0; MONTHLY_COUPON_COUNT],
            quarterly_autocall_schedule: [0; QUARTERLY_AUTOCALL_COUNT],
            next_coupon_index: 0,
            next_autocall_index: 0,
            offered_coupon_bps_s6: 500_000_000,
            coupon_barrier_bps: COUPON_BARRIER_BPS,
            autocall_barrier_bps: AUTOCALL_BARRIER_BPS,
            ki_barrier_bps: KI_BARRIER_BPS,
            missed_coupon_observations: 0,
            ki_latched: false,
            coupons_paid_usdc: 0,
            beta_spy_s12: 900_000_000_000,
            beta_qqq_s12: 400_000_000_000,
            alpha_s12: 50_000_000,
            regression_r_squared_s6: 950_000,
            regression_residual_vol_s6: 220_000,
            k12_correction_sha256: [0; 32],
            daily_ki_correction_sha256: [0; 32],
            settled_payout_usdc: 0,
            settled_at: 0,
            status: ProductStatus::Active,
        }
    }

    fn sample_regression() -> Regression {
        Regression {
            version: Regression::CURRENT_VERSION,
            beta_spy_s12: 900_000_000_000,
            beta_qqq_s12: 400_000_000_000,
            alpha_s12: 50_000_000,
            r_squared_s6: 950_000,
            residual_vol_s6: 220_000,
            window_start_ts: 0,
            window_end_ts: 0,
            last_update_slot: 0,
            last_update_ts: 0,
            sample_count: 63,
        }
    }

    #[test]
    fn build_midlife_inputs_uses_fixed_trading_day_schedules() {
        let inputs = build_midlife_inputs(
            &sample_policy_header(),
            &sample_terms(),
            &sample_regression(),
            [101_000_000, 102_000_000, 103_000_000],
            180_000,
            42,
        );

        assert_eq!(inputs.monthly_coupon_schedule[0], 21);
        assert_eq!(inputs.monthly_coupon_schedule[17], 378);
        assert_eq!(inputs.quarterly_autocall_schedule[0], 63);
        assert_eq!(inputs.quarterly_autocall_schedule[5], 378);
        assert_eq!(inputs.current_spy_s6, 101_000_000);
        assert_eq!(inputs.notional_usdc, 100_000_000);
        assert_eq!(inputs.now_trading_day, 42);
    }

    #[test]
    fn compute_nav_from_market_state_handles_terminal_par_case() {
        let mut terms = sample_terms();
        terms.next_coupon_index = 17;
        terms.next_autocall_index = 5;

        let valuation = compute_nav_from_market_state(
            &sample_policy_header(),
            &terms,
            &sample_regression(),
            [95_000_000, 94_000_000, 93_000_000],
            180_000,
            378,
        )
        .expect("terminal valuation");

        assert_eq!(valuation.nav.nav_s6, 1_000_000);
        assert_eq!(valuation.nav.remaining_coupon_pv_s6, 0);
        assert_eq!(valuation.nav.par_recovery_probability_s6, 1_000_000);
    }

    #[test]
    fn compute_nav_from_market_state_rejects_non_active_status() {
        let mut policy_header = sample_policy_header();
        policy_header.status = PolicyStatus::Settled;

        let err = compute_nav_from_market_state(
            &policy_header,
            &sample_terms(),
            &sample_regression(),
            [100_000_000, 100_000_000, 100_000_000],
            180_000,
            0,
        )
        .unwrap_err();

        assert_eq!(err, error!(FlagshipAutocallError::MidlifeNavUnavailable));
    }
}
