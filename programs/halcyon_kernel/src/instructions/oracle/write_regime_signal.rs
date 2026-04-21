use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::{state::*, KernelError};

const REGIME_WRITE_MIN_GAP_SECS: i64 = 18 * 60 * 60;
const MAX_REGIME_FVOL_S6: i64 = 1_000_000;
const REGIME_STRESS_THRESHOLD_S6: i64 = 600_000;
const SIGMA_MULTIPLIER_CALM_S6: i64 = 1_300_000;
const SIGMA_MULTIPLIER_STRESS_S6: i64 = 2_000_000;
const SIGMA_FLOOR_ANNUALISED_S6: i64 = 400_000;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WriteRegimeSignalArgs {
    pub product_program_id: Pubkey,
    pub fvol_s6: i64,
}

#[derive(Accounts)]
#[instruction(args: WriteRegimeSignalArgs)]
pub struct WriteRegimeSignal<'info> {
    pub keeper: Signer<'info>,

    #[account(seeds = [seeds::PROTOCOL_CONFIG], bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(seeds = [seeds::KEEPER_REGISTRY], bump)]
    pub keeper_registry: Account<'info, KeeperRegistry>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + RegimeSignal::INIT_SPACE,
        seeds = [seeds::REGIME_SIGNAL, args.product_program_id.as_ref()],
        bump,
    )]
    pub regime_signal: Account<'info, RegimeSignal>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<WriteRegimeSignal>, args: WriteRegimeSignalArgs) -> Result<()> {
    require_keys_eq!(
        ctx.accounts.keeper.key(),
        ctx.accounts.keeper_registry.regime,
        HalcyonError::KeeperAuthorityMismatch
    );
    let (regime, sigma_multiplier_s6, sigma_floor_annualised_s6) =
        derive_regime_signal_components(args.fvol_s6)?;

    let clock = Clock::get()?;
    let now = clock.unix_timestamp;
    let signal = &mut ctx.accounts.regime_signal;
    if signal.version == 0 {
        signal.version = RegimeSignal::CURRENT_VERSION;
        signal.product_program_id = args.product_program_id;
    } else {
        // K10 — strict monotonicity. `now` is the trusted clock, not keeper
        // input. Reject replays and stall-clock writes.
        require!(
            now > signal.last_update_ts,
            HalcyonError::OracleTimestampNotMonotonic
        );
        // L3 spec: reject writes if the previous signal is younger than
        // eighteen hours, regardless of the broader freshness cap products use
        // at quote time.
        require!(
            now.saturating_sub(signal.last_update_ts) >= REGIME_WRITE_MIN_GAP_SECS,
            HalcyonError::OracleRateLimited
        );
    }
    signal.fvol_s6 = args.fvol_s6;
    signal.regime = regime;
    signal.sigma_multiplier_s6 = sigma_multiplier_s6;
    signal.sigma_floor_annualised_s6 = sigma_floor_annualised_s6;
    signal.last_update_ts = now;
    signal.last_update_slot = clock.slot;
    Ok(())
}

fn derive_regime_signal_components(fvol_s6: i64) -> Result<(Regime, i64, i64)> {
    require!(
        (0..=MAX_REGIME_FVOL_S6).contains(&fvol_s6),
        KernelError::RegimeFvolOutOfRange
    );

    if fvol_s6 >= REGIME_STRESS_THRESHOLD_S6 {
        Ok((
            Regime::Stress,
            SIGMA_MULTIPLIER_STRESS_S6,
            SIGMA_FLOOR_ANNUALISED_S6,
        ))
    } else {
        Ok((
            Regime::Calm,
            SIGMA_MULTIPLIER_CALM_S6,
            SIGMA_FLOOR_ANNUALISED_S6,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_fvol() {
        let err = derive_regime_signal_components(-1).unwrap_err();
        assert!(
            format!("{err:?}").contains("RegimeFvolOutOfRange"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn rejects_fvol_above_one() {
        let err = derive_regime_signal_components(MAX_REGIME_FVOL_S6 + 1).unwrap_err();
        assert!(
            format!("{err:?}").contains("RegimeFvolOutOfRange"),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn derives_calm_regime_below_threshold() {
        let (regime, multiplier, floor) = derive_regime_signal_components(599_999).unwrap();
        assert_eq!(regime, Regime::Calm);
        assert_eq!(multiplier, SIGMA_MULTIPLIER_CALM_S6);
        assert_eq!(floor, SIGMA_FLOOR_ANNUALISED_S6);
    }

    #[test]
    fn derives_stress_regime_at_threshold() {
        let (regime, multiplier, floor) = derive_regime_signal_components(600_000).unwrap();
        assert_eq!(regime, Regime::Stress);
        assert_eq!(multiplier, SIGMA_MULTIPLIER_STRESS_S6);
        assert_eq!(floor, SIGMA_FLOOR_ANNUALISED_S6);
    }
}
