# Flagship Buyback Solvency

- Source config: `spy_qqq_iwm_factor_model_quarterly_recal_q65_cap500_daily`
- Legacy hedge-lab root: `/Users/dominic/Colosseum/halcyon-hedge-lab`
- Issued notes replayed: `4291` / `4400` candidates
- Note notional: `$100.00`
- Buyback rule: `min(KI cap, current note liability - 10% notional)`
- KI cap: `$70.00` on `$100.00` notional
- Current NAV source: note liability = daily pricer PV + accrued coupon liability
- Available funds at liquidation: dedicated note balance sheet assets after immediate adverse wrapper unwind
- Current production capital stack includes dedicated 12.5% junior first-loss capital per note

## Primary

- Buybacks always payable: `True`
- Liquidations: `452`
- Failures: `0`
- Minimum buffer: `$18.12`
- Minimum coverage ratio: `1.3319x`
- Total buyback paid: `$25279.59`
- Worst single day: `107` buybacks
- Worst 5-day window: `264` buybacks

## Stress

- Stress liquidation test: `25%` of live notes on days with worst-asset 24h return <= `-5%` or max 5d vol >= `100%`
- Buybacks always payable: `True`
- Liquidations: `708`
- Failures: `0`
- Minimum buffer: `$15.61`
- Minimum coverage ratio: `1.2458x`
- Total buyback paid: `$46481.85`
- Worst single day: `73` buybacks
- Worst 5-day window: `101` buybacks

## Notes

- The replay is coupled at the book level in the sense that liquidated notes are removed from all future days when portfolio concentration metrics are computed.
- The current flagship hedge architecture is per-note and dedicated-balance-sheet, so one note's buyback does not perturb surviving notes' hedge state under the current production replay design.
- Wrapper unwind stress remains assumption-driven because long stressed SPYX/QQQX/IWMX liquidity history is not directly observed in the research dataset.
