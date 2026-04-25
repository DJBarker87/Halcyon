use anchor_lang::prelude::*;
use halcyon_sol_autocall_quote::{
    generated::pod_deim_table::D as POD_DEIM_D,
    generated::pod_deim_table::POD_DEIM_TABLE_SHA256,
    midlife::{
        SOL_AUTOCALL_MIDLIFE_COS_TERMS, SOL_AUTOCALL_MIDLIFE_MARKOV_STATES,
        SOL_AUTOCALL_MIDLIFE_MATRIX_LEN,
    },
};

pub const OBSERVATION_COUNT: usize = 8;
#[cfg(feature = "integration-test")]
pub const OBSERVATION_INTERVAL_DAYS: u32 = 1;
#[cfg(not(feature = "integration-test"))]
pub const OBSERVATION_INTERVAL_DAYS: u32 = 2;
#[cfg(feature = "integration-test")]
pub const MATURITY_DAYS: u32 = OBSERVATION_COUNT as u32;
#[cfg(not(feature = "integration-test"))]
pub const MATURITY_DAYS: u32 = 16;
#[cfg(feature = "integration-test")]
pub const SECONDS_PER_DAY: i64 = 1;
#[cfg(not(feature = "integration-test"))]
pub const SECONDS_PER_DAY: i64 = 86_400;

pub const AUTOCALL_BARRIER_BPS: u64 = 10_250;
pub const COUPON_BARRIER_BPS: u64 = 10_000;
pub const KI_BARRIER_BPS: u64 = 7_000;
pub const NO_AUTOCALL_FIRST_N_OBS: u8 = 1;

pub const CURRENT_ENGINE_VERSION: u16 = 1;
pub const REDUCED_OPERATOR_LEN: usize = POD_DEIM_D * POD_DEIM_D;
pub const MIDLIFE_MATRIX_MAX_STEPS: usize = OBSERVATION_COUNT;
pub const MIDLIFE_MATRIX_N_STATES: usize = SOL_AUTOCALL_MIDLIFE_MARKOV_STATES;
pub const MIDLIFE_MATRIX_LEN: usize = SOL_AUTOCALL_MIDLIFE_MATRIX_LEN;
pub const MIDLIFE_MATRIX_MAX_VALUES: usize = MIDLIFE_MATRIX_MAX_STEPS * MIDLIFE_MATRIX_LEN;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub enum ProductStatus {
    Active,
    AutoCalled,
    Settled,
}

#[account]
#[derive(InitSpace)]
pub struct SolAutocallTerms {
    pub version: u8,
    pub policy_header: Pubkey,
    pub entry_price_s6: i64,
    pub autocall_barrier_s6: i64,
    pub coupon_barrier_s6: i64,
    pub ki_barrier_s6: i64,
    /// Scheduled unix timestamps for each of `OBSERVATION_COUNT` observation
    /// dates, relative to `issued_at`. Stored concretely so the keeper and
    /// the observation handler don't need to recompute the schedule.
    pub observation_schedule: [i64; OBSERVATION_COUNT],
    pub no_autocall_first_n_obs: u8,
    pub current_observation_index: u8,
    /// Quoted coupon per observation at SCALE_6 bps. Stored so later
    /// observation / settlement handlers can compute coupon accruals from the
    /// issued terms without rerunning the pricer.
    pub offered_coupon_bps_s6: i64,
    pub quote_share_bps: u16,
    pub issuer_margin_bps: u16,
    /// Cumulative coupons already paid on interim observations, plus the
    /// terminal coupon once the policy has been autocalled or settled.
    pub accumulated_coupon_usdc: u64,
    pub ki_triggered: bool,
    pub status: ProductStatus,
}

impl SolAutocallTerms {
    pub const CURRENT_VERSION: u8 = 1;

    /// Is the `i`th observation the final scheduled observation (maturity)?
    pub fn is_final_observation(&self, i: u8) -> bool {
        i as usize + 1 == OBSERVATION_COUNT
    }
}

#[account]
#[derive(InitSpace)]
pub struct SolAutocallReducedOperators {
    pub version: u8,
    pub sigma_ann_s6: i64,
    pub last_update_slot: u64,
    pub last_update_ts: i64,
    pub source_vault_sigma_slot: u64,
    pub source_regime_signal_slot: u64,
    pub pod_deim_table_sha256: [u8; 32],
    pub uploaded_v_len: u16,
    pub uploaded_u_len: u16,
    #[max_len(REDUCED_OPERATOR_LEN)]
    pub p_red_v: Vec<i64>,
    #[max_len(REDUCED_OPERATOR_LEN)]
    pub p_red_u: Vec<i64>,
}

impl SolAutocallReducedOperators {
    pub const CURRENT_VERSION: u8 = 1;

    pub fn matches_current_tables(&self) -> bool {
        self.pod_deim_table_sha256 == POD_DEIM_TABLE_SHA256
    }

    pub fn is_complete(&self) -> bool {
        self.p_red_v.len() == REDUCED_OPERATOR_LEN
            && self.p_red_u.len() == REDUCED_OPERATOR_LEN
            && self.uploaded_v_len as usize == REDUCED_OPERATOR_LEN
            && self.uploaded_u_len as usize == REDUCED_OPERATOR_LEN
    }
}

#[account]
#[derive(InitSpace)]
pub struct SolAutocallMidlifeMatrices {
    pub version: u8,
    pub sigma_ann_s6: i64,
    pub n_states: u16,
    pub cos_terms: u16,
    pub uploaded_step_count: u16,
    pub uploaded_lens: [u16; MIDLIFE_MATRIX_MAX_STEPS],
    pub step_days_s6: [i64; MIDLIFE_MATRIX_MAX_STEPS],
    pub last_update_slot: u64,
    pub last_update_ts: i64,
    pub source_vault_sigma_slot: u64,
    pub source_regime_signal_slot: u64,
    pub construction_inputs_sha256: [u8; 32],
    pub matrix_values_sha256: [u8; 32],
    #[max_len(MIDLIFE_MATRIX_MAX_VALUES)]
    pub matrices: Vec<i64>,
}

impl SolAutocallMidlifeMatrices {
    pub const CURRENT_VERSION: u8 = 1;

    pub fn is_complete(&self) -> bool {
        let count = self.uploaded_step_count as usize;
        count <= MIDLIFE_MATRIX_MAX_STEPS
            && self.n_states as usize == MIDLIFE_MATRIX_N_STATES
            && self.cos_terms == SOL_AUTOCALL_MIDLIFE_COS_TERMS
            && self.matrices.len() == count * MIDLIFE_MATRIX_LEN
            && self.construction_inputs_sha256 != [0u8; 32]
            && self.matrix_values_sha256 != [0u8; 32]
            && self.uploaded_lens[..count]
                .iter()
                .all(|len| *len as usize == MIDLIFE_MATRIX_LEN)
    }
}
