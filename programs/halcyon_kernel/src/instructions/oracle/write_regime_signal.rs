use anchor_lang::prelude::*;
use halcyon_common::{seeds, HalcyonError};

use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WriteRegimeSignalArgs {
    pub product_program_id: Pubkey,
    pub fvol_s6: i64,
    pub regime: u8,
    pub sigma_multiplier_s6: i64,
    pub sigma_floor_annualised_s6: i64,
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

    let regime = match args.regime {
        0 => Regime::Calm,
        1 => Regime::Stress,
        _ => return err!(HalcyonError::RegimeStale),
    };

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
        // Regime keeper is daily (per L3 plan §3.5). Enforce a minimum-gap
        // rate limit of `regime_staleness_cap_secs / 3` — anything more
        // frequent than that is a compromised keeper.
        let min_gap = ctx
            .accounts
            .protocol_config
            .regime_staleness_cap_secs
            .saturating_div(3);
        require!(
            now.saturating_sub(signal.last_update_ts) >= min_gap,
            HalcyonError::OracleRateLimited
        );
    }
    signal.fvol_s6 = args.fvol_s6;
    signal.regime = regime;
    signal.sigma_multiplier_s6 = args.sigma_multiplier_s6;
    signal.sigma_floor_annualised_s6 = args.sigma_floor_annualised_s6;
    signal.last_update_ts = now;
    signal.last_update_slot = clock.slot;
    Ok(())
}
