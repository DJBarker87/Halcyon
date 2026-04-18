# How the SOL Autocall Note Works — Plain-Language Economics

## What is it?

You deposit $1,000 USDC. Halcyon issues you a 16-day note tied to the price of SOL. Every 2 days, the system checks SOL's price and one of three things happens:

1. **SOL is up modestly** (above 102.5% of entry price) → You get your $1,000 back plus a coupon payment. Note ends early. This is called "autocall."

2. **SOL is roughly flat or up a little** (above 100% of entry but below 102.5%) → You collect a coupon for that period and the note continues. No early exit yet.

3. **SOL is down** (below 100% of entry) → No coupon this period. Note continues.

**A 2-day lockout** suppresses autocall at the first observation, so the earliest a note can exit is day 4. Coupons still pay at day 2 if SOL is above entry; knock-in is still monitored.

At the end of 16 days, if the note hasn't autocalled, you get your money back *unless* SOL crashed hard enough to trigger the knock-in barrier (dropped below 70% of entry at any point during the note). If the knock-in was triggered and SOL finishes below entry, you take a loss proportional to how far SOL fell.

## What actually happens in practice?

Based on replaying every possible entry window from August 2020 to March 2026 (2,042 possible notes):

**Most notes autocall early.** 68.1% of notes autocall before the 16 days are up. The earliest exit is day 4 (because of the lockout) — about 41% of notes exit on day 4. When a note autocalls at day 4, the buyer held a $1,000 position for 4 days, collected one or two coupons, and got their money back.

**The knock-in is rare.** About 6% of notes trigger the 70% knock-in barrier. This happens during sharp SOL crashes. When it triggers and SOL finishes below entry, the buyer loses money.

**Average note life is about 9 days**, even though the contract is 16 days. The bimodal pattern: about 41% exit on day 4 (the earliest allowed autocall) and about 34% go the full 16 days (no autocall).

## How much does the buyer make?

**Average per-note return: +1.65%** on the $1,000 deposit. That is roughly $16.50 earned per note.

**But what does that mean over a year?** If you buy a note, wait for it to finish, then immediately buy the next available note, and keep doing that through the entire replay period (August 2020 to March 2026), the compound annual growth rate is about **20.7%**.

That 20.7% is the honest number. A simple calculation of "1.65% per 9 days, annualized" overstates things because:
- You can't always buy a note immediately (issuance gates sometimes block)
- Note lives vary a lot (some are 4 days, some are 16)
- There are gaps between available notes

The product is available about 80% of trading days. On 20% of days, the system declines to issue because the economics don't work (volatility too low for a fair coupon, or too many existing notes are in trouble).

## How does the vault make money?

The vault is the other side of the trade. When you buy a note, the vault is underwriting your coupon and your principal protection.

**The vault earns money from three sources:**

1. **Retained coupon spread.** You get 75% of the fair coupon; the vault keeps 25%.

2. **Issuer margin.** An additional 50 basis points (0.50%) margin is charged per note.

3. **Hedge profits.** The vault hedges its SOL exposure by buying and selling spot SOL on Solana DEXes. When the hedge works well, it offsets some of the losses from paying out coupons and absorbing knock-in events.

**Mean vault profit: +$4.79 per $1,000 note** across 1,638 post-lockout notes. Across the full replay the vault is positive in every backtested year. This is not a passive yield and should not be marketed as an APR — it is a throughput metric that depends on issuance volume and the mix of autocall / dead-zone / knock-in outcomes.

## How does the hedging work?

This is the part that matters for operational design.

**The vault hedges by buying and selling spot SOL on-chain.** No perps, no options, no staking. Literal SOL tokens bought via Solana DEX swaps (e.g., Jupiter, Raydium).

Here is exactly what the hedge controller does:

1. **At note inception:** Buy SOL worth 50% of the note's delta exposure. For a $1,000 note, this means buying roughly $500 worth of SOL (adjusted by the model's delta calculation).

2. **Every 2 days (observation dates):** Recalculate the target hedge from the pricing model's delta surface. If the target has moved by more than 10% from the current position, rebalance. Cap the hedge at 75% of notional. Don't trade if the required trade is less than 1% of notional.

3. **Between observations:** Intraperiod checks are allowed when enabled by config (`allow_intraperiod_checks = true`), subject to a `max_rebalances_per_day` cap. The primary rebalance cadence is still observation dates.

4. **When the note ends:** Sell all remaining SOL hedge inventory.

**What this costs in practice:**
- Average 4.45 trades per note
- Average execution cost: $1.87 per note (swap fees + slippage + keeper bounty)
- Average turnover: 1.32x the note notional (i.e., total SOL bought and sold over the note's life is about 1.32x the $1,000 note)

**The hedge uses a separate capital sleeve.** The USDC needed to buy SOL for hedging comes from its own pool, separate from the coupon payment pool and the underwriting reserve. This way, a bad hedge trade doesn't directly eat into the coupon funding.

## What about staking / LSTs / yield on the hedge?

**Currently: nothing.** The hedge inventory is raw SOL sitting in a wallet. It is not staked, not deposited in a liquid staking protocol, and earns zero yield while held.

This is a potential improvement. If the SOL hedge inventory were held as a liquid staking token (like jitoSOL or mSOL), the vault would earn approximately 6-7% APY on the hedged position. On a note with an 8-day average life and a ~$500 average hedge position, this works out to roughly $0.50-0.75 extra per note — small but meaningful across thousands of notes.

The reason it's not implemented yet: liquid staking adds operational complexity. Unstaking takes 2-3 Solana epochs (~4-6 days), and the hedge needs to be able to sell quickly when rebalancing. A production implementation would need to keep a raw-SOL execution buffer for fast trades and only stake the "slow core" portion of the hedge that doesn't need to move quickly.

Similarly, the idle USDC reserve (the portion not currently backing active notes) could earn yield in a stablecoin product like USDY (~4.5% APY). Again, not implemented yet.

## Where does the money come from?

This is the question people should ask about any structured product.

**The buyer's return comes from the vault.** The vault is selling you a payoff structure — specifically, it is selling you coupons (which cost the vault money) and principal protection above the knock-in barrier (which costs the vault money when SOL drops). The buyer is paying for this through the coupon haircut (getting 75% of fair value) and the issuer margin (50bp).

**The vault's return comes from:**
- Keeping the coupon spread and margin (the "house edge")
- Hedging skillfully enough that the spot SOL position offsets most of the liability
- Only issuing notes when the model says the economics work (the "no quote" feature)

**Nobody is getting free money.** The buyer earns 20.7% CAGR, but they are exposed to tail risk: roughly 6% of the time, the knock-in triggers and the buyer can lose a meaningful chunk of principal. The vault earns +$4.79 per note on average, positive in every backtested year, but it is taking on underwriting risk — in bad crash scenarios, individual notes lose money and the vault relies on the knock-in-retained principal events to stay profitable.

The system is designed so that both sides are better off than the naive alternative:
- The buyer is better off than holding SOL (which has higher drawdowns)
- The vault is better off than holding USDC (which earns less)
- The "no quote" gate prevents issuance when neither side would benefit

## The economics in one sentence

The buyer deposits USDC, earns ~1.65% per note (roughly 21% annualized by reinvesting) with ~6% tail risk of principal loss, while the vault underwrites the payoff by buying and selling spot SOL on-chain and keeping the 25% coupon spread and 50bp margin, earning +$4.79 per $1,000 note on average and positive in every backtested year.
