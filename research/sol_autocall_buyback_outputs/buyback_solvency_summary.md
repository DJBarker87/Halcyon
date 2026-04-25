# SOL Buyback Solvency

- Row: `CURRENT_V1_HEDGED_BALANCED`
- Source step ledger: `/Users/dominic/Colosseum/research/sol_autocall_hedged_sweep/outputs/parity_hedge_step_ledger.csv`
- Replay method: `coupled book-level replay over live hedge states`
- Notes in ledger export: `1550`
- Underlying window: `2020-08-11` to `2026-03-20`
- Buyback formula overlay: `min(KI_level - 10%, current_capital_mark - 10%)`
- KI cap: `$600.00` on `$1000` notional
- Lending trigger: initial LTV `70%`, liquidation LTV `85%`
- Stress liquidation test: `25%` of live notes on days with 24h return <= `-5%`

## Primary

- Liquidated notes: `2` / `1550` (`0.13%`)
- Buybacks always payable: `True`
- Failure count: `0`
- Min buffer: `$99.39`
- Min coverage ratio: `1.2018`
- Total buyback paid: `$976.64`
- Total unwind cost: `$0.82`
- Worst single day: `1` buybacks
- Worst 5d liquidation window: `1` notes, `2021-06-17` -> `2021-06-21`

## Stress Concentration

- Liquidated notes: `86` / `1550` (`5.55%`)
- Buybacks always payable: `True`
- Failure count: `0`
- Min buffer: `$224.04`
- Min coverage ratio: `1.3734`
- Total buyback paid: `$51600.00`
- Total unwind cost: `$67.25`
- Worst single day: `3` buybacks
- Worst 5d liquidation window: `6` notes, `2026-01-27` -> `2026-01-31`

## Assumptions

- The replay is coupled at the book level: buybacks fire inside the daily book loop and liquidated notes are removed immediately from all future days.
- The production row uses separate sleeves and note-local hedge state, so removing a liquidated note does not alter surviving notes' hedge decisions.
- `current_capital_mark = notional + hedge_cash + hedge_inventory * close - coupons_paid`
- Unwind cost uses the production sqrt-impact curve from `halcyon_sol_autocall_quote::sol_swap_cost` with 10 bps base fee, coefficient 25, liquidity proxy $250k, and a 3x multiplier in stress.

