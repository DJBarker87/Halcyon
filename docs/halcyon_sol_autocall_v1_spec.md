# Halcyon SOL Autocall v1 — Locked Candidate

## Status

This document freezes the current best **production-minded** SOL autocall candidate based on the latest 6%+ dynamic CVault yield sweep.

It also records the strongest **headline/demo** variant separately, so product, design, and pitch work do not get conflated.

---

## Product family

**SOL Athena autocallable note**

- Underlying: SOL
- Settlement: USDC cash-settled
- Principal: escrowed per note
- Coupon funding: separate coupon/liquidity vault
- Observation cadence: every 2 days
- Coupon memory: none
- Coupon barrier: 100% of initial spot
- Autocall: observation-date only
- Knock-in: path-dependent, using hourly-low touch proxy in backtesting
- Downside after knock-in: bank-standard downside from initial if final is below initial

This is an **autocall**, not a buffer/floor product.

---

## Locked v1 production candidate

### Core terms

- **Tenor:** 16 days
- **Observation cadence:** every 2 days
- **Autocall barrier:** 102.5% of initial SOL spot
- **Knock-in barrier:** 70.0% of initial SOL spot
- **Quoted coupon:** 75% of fair
- **Explicit issuer margin:** 50bp per note
- **Issuance gate:** issue only when **fair coupon >= 0.50% per observation**

### Historical operating metrics

- **Dynamic CVault yield:** 8.66% per year
- **Full-strip CVault yield:** 14.37% per year
- **Median coupon:** 1.538% per observation
- **Issuance fraction:** 75.0%
- **Autocalled fraction:** 72.4%
- **Profitable-user fraction:** 78.7%
- **Dynamic reserve:** 61.1% of max active notional
- **P05 user total return:** -26.18%

### Why this row wins

This is the first row that looks like both:

1. a **real product** for users, and
2. a **real business** for the coupon vault.

It clears the 6% dynamic CVault yield hurdle while still offering a meaningful coupon and strong user history.

---

## Strongest headline / demo variant

This is **not** the main production row. It is the cleaner pitch/demo version.

### Core terms

- **Tenor:** 16 days
- **Observation cadence:** every 2 days
- **Autocall barrier:** 102.5% of initial SOL spot
- **Knock-in barrier:** 70.0% of initial SOL spot
- **Quoted coupon:** 77.5% of fair
- **Explicit issuer margin:** 75bp per note
- **Issuance gate:** issue only when **expected PnL >= 75bp**

### Historical operating metrics

- **Dynamic CVault yield:** 7.71% per year
- **Median coupon:** 1.669% per observation
- **Issuance fraction:** 68.1%
- **Profitable-user fraction:** 78.3%

### Use

Use this row for:

- pitch decks
- UI mockups
- demo narration
- user-facing examples

Do **not** use it as the default production row unless later sweeps justify it.

---

## Why the product is gated

The latest sweep showed that **no always-on row cleared the 6% dynamic CVault yield hurdle**.

That means the honest framing is:

> this autocall is a **state-dependent issuance product**, not an always-available note.

In plain English:

- when market conditions are favorable, Halcyon issues the note;
- when conditions are not favorable, Halcyon does not issue it.

This is a feature, not a bug. It avoids writing bad business just to stay live all the time.

---

## Plain-English user proposition

### Halcyon SOL Autocall

Earn a fixed coupon every 2 days while SOL stays healthy, with early redemption if SOL rallies modestly.

- Deposit USDC
- If SOL is at or above your entry level on an observation date, you earn the coupon
- If SOL is at or above the autocall level, the note redeems early and returns your principal
- If SOL crashes through the knock-in barrier and finishes below entry at maturity, your principal is at risk

This is for users who are:

- bullish to neutral on SOL over a short horizon
- happy to trade unlimited upside for frequent coupon opportunities
- comfortable with real downside risk in bad crash scenarios

---

## Payoff summary

### During the life of the note

Every 2 days, the note checks SOL against two levels:

- **100% of initial:** coupon condition
- **102.5% of initial:** autocall condition

### If coupon condition is met

- that observation’s coupon is paid

### If autocall condition is met

- coupon for that observation is paid
- note redeems early
- principal is returned in USDC
- note ends

### If the note reaches maturity without autocalling

#### If knock-in was never touched

- principal returns at par in USDC
- final coupon only pays if the maturity observation is at or above initial

#### If knock-in was touched and final is below initial

- principal redemption is reduced in line with SOL performance from initial
- this is the real downside scenario

---

## Product design logic

This structure now rests on the corrected accounting model:

- **principal is escrowed per note**;
- **coupon/liquidity funding is separate**;
- the main operating capital problem is **coupon liquidity**, not pretending principal itself is underwriting capital.

That correction is what made the autocall viable.

---

## Risk disclosure

This is **not** a principal-protected savings product.

Users can suffer substantial losses if:

- SOL touches the knock-in barrier during the note life, and
- SOL finishes below initial at maturity.

The historical **P05 total return of -26.18%** on the frozen row makes this explicit: the product can deliver strong coupon economics in normal/choppy regimes, but it carries real crash risk.

---

## Internal freeze decision

### Freeze as production-minded v1

**16d / AC 102.5% / KI 70% / quote 75% fair / 50bp margin / fair coupon >= 0.50% gate**

### Keep as headline/demo row

**16d / AC 102.5% / KI 70% / quote 77.5% fair / 75bp margin / expected PnL >= 75bp gate**

---

## Next workstreams

1. frontend product card and payoff explainer
2. final term sheet wording
3. issuance-gate implementation logic
4. coupon-vault monitoring and reserve dashboard
5. pitch copy aligned to gated issuance rather than always-on availability

