# IL Hedge Challenger

## Decision

Keep IL Protection on the frozen launch path:

- cached `hedge_d10_c70.npy`
- `max(EWMA_45d_SOL * 1.30, 0.40)`
- explicit `x1.10` launch load

Do **not** promote a mixture-NIG or state-overlay engine into the default path right now.

## What Was Added

- Research script: `research/il_hedge_challenger.py`
- Generated result memo: `research/il_hedge_challenger_results.md`

The script wraps the existing launch table with:

1. a smooth two-state mixture between baseline sigma and stressed sigma,
2. an fvol-based state-weight adapter,
3. explicit tail-load add-on experiments.

## Why The Challenger Stays Research-Only

The mixture wrapper improves stress-state seller economics, but not by enough to justify a new production state model:

- stress loss ratio improves materially,
- buyer downside help worsens,
- worst-decile seller improvement is modest,
- and prior overlay work in the repo already failed the quality-bar test by charging too much too often.

The simpler next lever is an explicit tail-load or surcharge on top of the existing launch table if the seller side needs more protection.

## Rerun

```bash
cd /Users/dominic/Colosseum
make il-hedge-challenger
```
