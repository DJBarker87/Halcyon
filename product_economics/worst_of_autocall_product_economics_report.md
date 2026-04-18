# Worst-Of Autocall — Full Product Economics Report

**Candidate:** SPY/QQQ/IWM 18-month worst-of with monthly coupon / quarterly autocall split
**Backtest:** One-factor NIG with Gaussian residuals, per-quarter recalibration (77 quarterly calibrations from 2007-06-29 to 2026-04-10, each on trailing 252 trading days), proxy hedge (IWM via SPY/QQQ regression)
**Data:** SPY, QQQ, IWM daily, April 2006 to April 2026 (5,031 bars)
**Cadence tested:** 210 monthly-spaced windows (206 issued) + 4,400 daily-spaced windows (4,291 issued, production scenario)

---

## 1. Product Structure

A buyer deposits $100 USDC. The system issues an 18-month note tied to the worst-performing normalised path across SPY, QQQ, and IWM. Coupons are checked monthly. Autocall is checked quarterly. Knock-in is monitored continuously.

| Parameter | Value |
|---|---|
| Underlyings | SPY, QQQ, IWM (worst-of) |
| Tenor | 378 trading days (~18 months) |
| Coupon observation | Every 21 trading days (monthly), 18 dates |
| Autocall observation | Every 63 trading days (quarterly), 6 dates |
| Autocall barrier | 100% of entry (all three names must be at or above) |
| Coupon barrier | 100% of entry (worst performer must be at or above) |
| Knock-in barrier | 80% of entry (any name breaching at any time) |
| Coupon policy | Memory — missed coupons accumulate, paid on next eligible date |
| Quote share | 65% of model fair coupon |
| Issuer margin | 100 basis points per note |
| Fair coupon floor | 50 bps per observation (no-quote below this) |
| Fair coupon ceiling | 500 bps per observation (no-quote above this) |
| Pricing model | One-factor NIG with Gaussian residuals (X_i = m_i·t + ℓ_i·F + ε_i) |
| Recalibration cadence | Quarterly, trailing 252 trading days |
| Hedge | Proxy — IWM delta projected into SPY/QQQ via rolling 252d regression |
| Junior first-loss | 12.5% of notional |
| Vault lock per note | $113.50 ($100 principal + $12.50 junior + $1.00 issuance fee) |

**Why three names:** A single-name autocall (SPY alone) has lower fair coupons because SPY is less volatile. The worst-of across three names sells correlation and idiosyncratic vol — when one name underperforms, the note pays a lower coupon or triggers KI, which generates a richer premium. The buyer accepts more payoff risk in exchange for a higher coupon.

**Why the split schedule:** TradFi shelf autocalls (iCapital, Halo, Calamos CAIE/CAIQ) use monthly coupon with quarterly or annual autocall. Monthly coupons give the buyer regular income. Quarterly autocall prevents the note from terminating too quickly — the minimum life is 63 trading days, giving the vault more fee accrual time. The split halves worst drawdown versus the equivalent quarterly/quarterly structure at the same quote share.

**Why the proxy hedge:** There is no on-chain IWM wrapper token with sufficient liquidity. The vault hedges SPY and QQQ directly via on-chain wrappers, and projects IWM delta exposure onto SPY/QQQ using a rolling beta regression (IWM ≈ 1.14·SPY − 0.01·QQQ, R² ~80%). This costs ~70 bps of occupied-capital return versus a hypothetical direct 3-leg hedge, but avoids the need for a third wrapper.

**Why the 500 bps ceiling:** The pricer signals regime stress by asking for a high fair coupon. Windows where fair coupon exceeds 500 bps/obs correspond to extreme vol regimes entering GFC (Oct–Dec 2008), 2015 summer vol, 2011 Euro crisis, 2022 rate shock. Admitting these windows doubles worst drawdown; excluding them costs only 2% of issuance (4/210 monthly windows, ~109/4400 daily windows).

**Why per-quarter recalibration:** A single full-sample calibration systematically mispriced quiet regimes and underpriced stress regimes. Trailing-252d quarterly calibrations let factor NIG shape (α_F, β_F) and loadings (ℓ_SPY, ℓ_QQQ, ℓ_IWM) track regime changes. At q=0.65 this cut vault drawdown from −34.7 (static) to −17.8 (monthly quarterly-recal) while leaving buyer IRR essentially unchanged.

---

## 2. What Happens to Each Note

Three outcomes are possible:

**Autocall (~83% of notes).** All three names are at or above 100% of entry at a quarterly observation. The buyer gets full principal ($100) plus all coupons earned (including accumulated memory coupons). Average life: ~145 trading days (~7 months). The earliest autocall is day 63 (quarter 1).

**Full term, no KI (~4% of notes).** The note runs the full 378 days without any name dropping below 80% or all three rising above 100% at a quarterly check. The buyer gets full principal ($100) plus whatever coupons were earned.

**Knock-in (~14% of notes with principal loss).** At least one name dropped below 80% of entry at some point. If the worst performer finishes below entry at maturity, the buyer takes a principal loss equal to the worst performer's decline. KI triggers on ~22% of notes, but only ~14% result in actual principal loss — in the other 8%, the worst performer recovers above entry by maturity.

---

## 3. Buyer Economics

Headline (monthly-cadence backtest, 206 notes issued):

| Metric | Value |
|---|---|
| Average annualised IRR | **+8.35%** |
| Annualised quoted coupon | 14.9% |
| Average fair coupon | ~200 bps per monthly observation |
| Average quoted coupon | ~130 bps per monthly observation |
| Notes with any principal loss | ~13% |
| KI trigger rate | ~22% |
| Average note life | ~145 trading days (~7 months) |

Daily-cadence stress (4,291 notes issued):

| Metric | Value |
|---|---|
| Average annualised IRR | **+8.49%** |
| Annualised quoted coupon | 15.1% |
| Autocall rate | 83.1% |
| KI trigger rate | 21.7% |
| Loss rate | 14.4% |
| Average note life | 145 days |

The gap between quoted coupon (~15%) and realised IRR (~8.5%) comes from three sources: missed monthly coupons (worst performer below 100% at observation), principal losses from KI events, and the issuance fee.

### What the Buyer Gets Each Month

On each monthly observation date, the system checks whether the worst-performing name is at or above 100% of entry:

- **If yes:** The buyer receives the quoted coupon (~$1.30 on $100 notional) plus all accumulated missed coupons.
- **If no:** The coupon is missed and added to the memory. No cash flow.

Over an average note life of ~7 months with up to 18 possible monthly observations before autocall, the buyer typically receives 4-6 coupon payments before the note autocalls at a quarterly date.

### The Buyer's Risk

The worst case is a knock-in event where the worst performer (usually IWM in a drawdown) drops below 80% and stays below entry at maturity. In the backtest, 13–14% of notes had principal loss. The left tail is real and driven by equity drawdowns that breach the 80% KI barrier — and because the one-factor NIG captures joint co-movement in the left tail, the tail losses are deeper than a Gaussian copula would have suggested. The buyer's P1-P5 returns are meaningfully negative.

---

## 4. Vault Economics (Hedged, Proxy, Quarterly-Recal, 500 bps Ceiling)

Per-note (both cadences close):

| Metric | Monthly | Daily |
|---|---|---|
| Vault occupied-capital return | **+5.17%** | **+5.63%** |
| Mean vault P&L per note | $3.48 | $3.82 |
| CVaR(5%), per note | −5.96 | −6.13 |
| Worst single-note vault P&L | ~−$14 | −$18.87 |
| Insolvency rate | 0% | 0% |

Book-level drawdown (daily cadence, 4,291 notes over 19 years):

| Metric | Value |
|---|---|
| Peak concurrent notes | 317 |
| Median concurrent notes | 120 |
| Peak concurrent book notional | ~$31,700 |
| Worst cumulative drawdown | −$265.5 |
| DD as % of peak concurrent book | ~0.84% |
| Total lifetime cumulative P&L | ~$16,400 |
| DD as % of lifetime earnings | ~1.6% |

Monthly-cadence book-level DD is −$17.8 (peak concurrent ~18 notes). Daily cadence scales the raw DD roughly 15× because the concurrent book is ~17× larger — but DD per dollar of in-flight notional is in fact smaller under daily issuance because entry-date diversification dilutes crisis clustering.

### How the Vault Makes Money

The vault's income comes from four streams:

**Retained coupon haircut (primary).** The vault issues at 65% of fair coupon, retaining 35% as underwriting margin. At an average fair coupon of ~200 bps per month, the vault retains ~70 bps per month per note.

**Issuer margin (fixed).** 100 bps of notional charged at issuance. On a $100 note, that's $1.00 upfront.

**Reserve yield.** 4% APR on escrowed notional over the note's life (accrued on idle cash only).

**KI residual.** When a note settles at a loss to the buyer, the vault retains the difference between par and the settlement value, net of hedge P&L.

### How the Vault Loses Money

**Dead-zone bleed.** When the worst performer drops 0-20% but stays above the 80% KI barrier, the hedge (which is long the underlyings) loses money while the vault still owes the buyer full principal. This is the primary source of vault losses on individual notes.

**Proxy hedge residual.** The IWM-specific component of returns that is not captured by the SPY/QQQ proxy. In windows where IWM diverges from its regression prediction (R² ~80%, residual vol ~9.8%), the proxy hedge over- or under-hedges, creating variance.

**Hedge execution costs.** DEX swap fees, slippage, and keeper costs. Average execution cost is ~$1.50-2.00 per note.

### The Risk Profile

The vault's per-note CVaR(5%) is about −$6 at either cadence. At daily issuance with peak concurrent exposure of $31.7K, the worst 19-year cumulative drawdown is −$265 (0.84% of peak book, 1.6% of lifetime earnings). There are no insolvency events in the 20-year backtest — the 12.5% junior first-loss tranche absorbs the drawdown without breaching senior capital.

---

## 5. Hedging

The vault hedges using only two on-chain wrapper tokens (SPYX and QQQX). IWM exposure is projected into SPY and QQQ using a rolling regression.

| Metric | Value |
|---|---|
| Hedge legs | 2 (SPY wrapper, QQQ wrapper) |
| IWM proxy | Rolling 252d OLS: IWM ~ β₁·SPY + β₂·QQQ (zero intercept) |
| Proxy R² (mean) | 79.6% |
| Proxy R² (P10) | 66.0% |
| Residual vol | 9.8% annualised |
| Execution policy | delta_band_raw, daily check |
| Band fraction | 0.5% of notional per leg |
| Cost of proxy vs direct | ~70 bps occupied-capital return |

**What the proxy regression means:** On average, $1 of IWM delta is hedged by buying ~$1.14 of SPY and selling ~$0.01 of QQQ. The regression captures 80% of IWM's variance. The remaining 20% is unhedged small-cap-specific risk that shows up as additional P&L variance.

---

## 6. Issuance

| Metric | Monthly (21d stride) | Daily (1d stride) |
|---|---|---|
| Possible entry windows | 210 | 4,400 |
| Notes issued | 206 | 4,291 |
| Issuance rate | 98.1% | 97.5% |

The issuance gate rejects windows where the model fair coupon falls outside the 50–500 bps per observation range. The 500 bps ceiling excludes ~2% of windows — those are the extreme vol regimes entering GFC, 2015 summer, 2011 Euro, and 2022 rate shock. Excluding them materially reduces vault drawdown without changing per-note economics on the admitted set.

---

## 7. The Split Schedule — Monthly Coupon / Quarterly Autocall

The product uses a split observation schedule that separates coupon checks (monthly) from autocall checks (quarterly). This matches the dominant TradFi shelf autocall structure.

The split structure:
- **Improves the vault's tail risk** — consistent with the earlier Gaussian-copula comparison, CVaR and worst drawdown halve versus a pure quarterly/quarterly structure at the same quote share.
- **Gives the buyer monthly income** — 18 payment dates instead of 6.
- **Slightly extends note life** — minimum 63 days before autocall vs immediate in pure quarterly.
- **Costs ~30 bps of vault return** — acceptable given the tail improvement.

### How Memory Coupon Works in the Split

The buyer has 18 monthly coupon observations. If the worst performer is below 100% on months 2 and 3 but recovers by month 4, the month-4 payment includes the month-2 and month-3 missed coupons plus the month-4 coupon (3× the monthly rate). On autocall (quarterly), all remaining accumulated coupons are paid with the principal redemption.

---

## 7a. Year-by-Year Economic Breakdown (Daily Issuance)

Cohorts by **issue year** from the 4,291-note daily-cadence backtest. "n" is notes issued (not calendar days; ~252 per full year). "vault_$/note" is mean vault P&L per note issued that year. "total_$" is the sum across that year's cohort. "life" is mean trading days each cohort's notes lived before autocall/maturity.

| year | n | buyer IRR | vault $/note | total $ | loss | KI | AC | life |
|---|---|---|---|---|---|---|---|---|
| 2007 | 182 | **−24.80%** | +$0.38 | +$69.5 | 85% | 85% | 15% | 331 |
| 2008 | 214 | −1.55% | +$4.34 | +$928.4 | 67% | 79% | 31% | 297 |
| 2009 | 227 | +26.59% | +$2.99 | +$677.6 | 0% | 3% | 100% | 71 |
| 2010 | 252 | +12.84% | +$3.27 | +$825.3 | 0% | 2% | 100% | 98 |
| 2011 | 252 | +13.00% | +$3.90 | +$984.0 | 15% | 44% | 78% | 171 |
| 2012 | 250 | +10.65% | +$2.11 | +$527.3 | 0% | 0% | 100% | 112 |
| 2013 | 252 | +4.95% | +$3.20 | +$805.6 | 0% | 0% | 100% | 67 |
| 2014 | 252 | +4.74% | +$4.67 | +$1,175.9 | 1% | 1% | 99% | 109 |
| 2015 | 252 | +10.05% | **−$0.41** | **−$102.1** | 9% | 46% | 69% | 215 |
| 2016 | 252 | +7.87% | +$2.35 | +$591.6 | 0% | 0% | 100% | 78 |
| 2017 | 232 | +2.18% | +$2.70 | +$627.4 | 0% | 0% | 100% | 79 |
| 2018 | 242 | +5.31% | +$7.21 | +$1,743.9 | 24% | 29% | 72% | 169 |
| 2019 | 252 | +10.85% | +$4.44 | +$1,119.9 | 0% | 11% | 100% | 114 |
| 2020 | 236 | +19.61% | +$4.84 | +$1,143.2 | 0% | 20% | 100% | 93 |
| 2021 | 252 | +4.28% | +$6.06 | +$1,527.1 | 48% | 48% | 52% | 224 |
| 2022 | 251 | +13.52% | +$4.99 | +$1,253.5 | 31% | 38% | 61% | 237 |
| 2023 | 250 | +12.36% | +$5.28 | +$1,320.1 | 0% | 0% | 100% | 105 |
| 2024 | 191 | +12.51% | +$6.03 | +$1,151.9 | 0% | 2% | 100% | 86 |

**Read-out by regime:**

- **2007 (pre-GFC entries):** the only year with a negative buyer average. Notes issued Apr 2006–Apr 2007 lived into the GFC, hit KI, and settled below entry. Vault still eked out a tiny positive per note because retained coupons and hedge P&L offset most of the terminal loss.
- **2008 (GFC year):** 67% of notes lost principal, but buyer IRR was only mildly negative because notes paid coupons for months before the crash. Vault earned +$4.34/note — the coupon haircut + hedge carried the vault through the worst equity regime in the sample. Longest average life (297d) reflecting delayed autocall under a bear market.
- **2009 (V-recovery):** best buyer year (+26.6%). All notes autocalled at the first quarterly check (71d average life). Vault returned +$2.99/note despite short lives.
- **2010–2014 (benign):** vault steady at +$2–5/note, buyer 5–13%, loss rate near zero. Classic autocall income stream.
- **2015 (summer vol):** **the only losing vault year** (−$0.41/note, −$102 total). China devaluation + oil collapse produced enough chop to trigger KIs on 9% of notes without generating offsetting autocall income.
- **2016–2017 (low-vol):** bull-market calm. Vault stable, buyer yields low because fair coupons were low.
- **2018 (Q4 selloff):** vault's best year per note (+$7.21). 24% of notes lost principal in the Dec-2018 drawdown, but the high fair coupons going in meant the coupon haircut retained more margin.
- **2019–2020 (pre- and post-COVID):** buyer +10% / +19.6%. COVID-era notes entered at elevated vol, benefited from rapid recovery, and autocalled quickly.
- **2021 (post-stimulus chop):** buyer only +4.3% despite high quoted coupons. 48% of notes lost principal — these were issued at late-stimulus peaks and caught the 2022 selloff. Vault still +$6.06/note because retained coupons and hedge carry dominated.
- **2022 (bear market):** vault +$4.99/note. High fair coupons generated strong retention; 31% of notes lost principal but the coupon stream paid for it.
- **2023–2024 (AI rally):** back to calm. Quick autocalls, minimal losses.

**Sum across 2007–2024:** vault P&L cumulative ≈ +$16,400. The only negative year was 2015 (−$102). Every crisis year (2008 GFC, 2020 COVID, 2022 bear) ended positive for the vault because buyers entering those regimes paid rich fair coupons that the retained haircut + hedge carry captured.

---

## 8. What This Backtest Does and Does Not Show

### It shows:
- Full production pipeline: one-factor NIG with quarterly recalibration → fair coupon → quote economics → 500 bps issuance gate → proxy hedge simulation → full cash flow accounting
- 20 years of equity history covering the GFC, COVID crash, 2022 bear market, and multiple recovery regimes
- Per-note economics that hold up across both sparse-entry (210 monthly windows) and dense-entry (4,400 daily windows) cadences
- Book-level behaviour with peak concurrent exposure of 317 notes (daily) without vault insolvency
- The effect of the 500 bps ceiling on regime-stress filtering
- The cost of proxy-hedging IWM through SPY/QQQ (~70 bps)

### It does not show:
- Real on-chain wrapper basis during stress (uses a modelled dynamic basis)
- The effect of SPY/QQQ wrapper liquidity constraints on very large hedge positions
- Intraday price behavior finer than daily candles for KI monitoring
- The interaction with the shared underwriting vault (IL Protection, SOL Autocall)
- Wrapper-to-reference tracking during flash crashes

---

## 9. Assumptions

| Assumption | Value | Source | Risk |
|---|---|---|---|
| Factor model | One-factor NIG + Gaussian residuals | Quarterly calibrations in `output/factor_model/quarterly/` (77 JSONs) | Each quarter uses a trailing-252d fit |
| Recalibration cadence | Quarterly, trailing 252 trading days | Per-window switch to most-recent calibration | Short (252d) sample can produce near-singular residual covariance; repaired via eigenvalue clipping |
| Factor loadings | Vary per quarter (e.g., SPY 0.49–0.57, QQQ 0.53–0.63, IWM 0.58–0.69) | Joint calibration per quarter-end | Tracks regime changes, but not breakdown across distinct crisis types |
| Residual covariance | 3D Gaussian Σ_ε · t | From calibration | Thin residual tails — joint heavy-tailed risk is carried by the common factor |
| Proxy regression | Rolling 252d OLS, zero intercept | Daily log-return regression | R² drops to 66% at P10; IWM-specific moves are unhedged |
| DEX swap fee | 4 bps | Conservative assumption | Real Jupiter/Raydium routes may differ |
| Slippage | sqrt impact model, $50M daily liquidity | Assumed | Overstates cost in normal markets |
| Wrapper basis | Dynamic model with vol/drawdown sensitivity | Calibrated to SPY/QQQ proxy basis | Not validated against real on-chain wrapper data |
| Reserve yield | 4% APR | Conservative fixed rate on idle cash only | Actual DeFi lending rates vary |
| Junior first-loss | 12.5% of notional | Configuration choice | Sufficient for all backtest regimes; untested beyond historical extremes |
| Vault lock per note | $113.50 | $100 principal + $12.50 junior + $1.00 issuance fee | Held for full note life (~145 days avg) |

---

## 10. The Product in One Page

**For the buyer:** Deposit $100 USDC, earn monthly income at ~15% annualised quoted rate when all three equity indices (SPY, QQQ, IWM) stay healthy. Missed coupons accumulate and pay on the next good month. If all three indices are above entry at a quarterly check, the note autocalls at par with all accumulated coupons — average life is about 7 months. Realised return after missed coupons and KI losses averages 8.4% annualised. ~86% of notes return principal. ~14% lose money, with the worst outcomes in sustained equity drawdowns where all three names fall together.

**For the vault:** Underwrite worst-of equity risk, earn 5.2–5.6% annualised on occupied capital (~$113.50 locked per $100 note). The primary income is the 35% retained coupon haircut plus the 100 bp issuance fee. The hedge uses only SPY and QQQ wrappers on-chain — no IWM wrapper needed. The proxy hedge costs ~70 bps versus direct hedging but avoids any third-token dependency. Tail risk is bounded — per-note CVaR(5%) is about −$6, worst cumulative book drawdown over 19 years of daily issuance is ~0.84% of peak concurrent notional. The vault has never been insolvent in 20 years of backtested history. The 12.5% junior tranche absorbs these drawdowns without breaching senior capital.

**For the protocol:** The product runs entirely on Solana — pricing, issuance, hedging, and settlement. No external venues, no bridges, no counterparty risk. The one-factor NIG pricer runs on-chain inside the flagship quote program. The issuance gate refuses to issue when fair coupon is outside the 50–500 bps safe range. The split coupon/autocall schedule matches the dominant TradFi structure while halving the vault's worst drawdown versus a uniform quarterly schedule. Per-quarter recalibration lets the pricer track regime changes without a full redeployment.

**The honest tension:** The buyer is selling worst-of correlation risk. When equity markets sell off together (GFC, COVID), all three names drop simultaneously, KI triggers, and the buyer takes a principal loss. The memory coupon provides a safety valve — accumulated coupons still pay on recovery — but cannot eliminate the downside. The 80% KI barrier means the buyer is exposed to crashes deeper than 20% on any single name. The ~8.4% realised IRR is the compensation for bearing that risk; the heavier the joint left tail, the more the buyer is compensated and the more the vault charges for carrying it.
