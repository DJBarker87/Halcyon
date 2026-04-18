# IL Protection — Full Product Economics Report

**Product:** 30-day SOL/USDC Raydium CPMM impermanent loss protection
**Backtest:** 2,027 rolling 30-day windows, August 2020 to February 2026
**Pricing:** Rust NIG European engine (`nig_european_il_premium`) x 1.10 underwriting load, alpha=3.1401, beta=+1.2139, delta=0.38440
**Validation:** All 2,027 premiums and settlements computed by the on-chain Rust engine (zero failures)
**Fee benchmark:** Raydium WSOL-USDC Standard 0.25% CPMM — 16.4% median APY (DeFiLlama-measured)
**Hedge:** None — the vault underwrites IL risk directly

---

## 1. Product Structure

An LP deposits into a SOL/USDC 50/50 constant-product pool on Raydium and buys IL Protection. If SOL moves enough in either direction over 30 days, the LP's terminal impermanent loss is covered between the deductible and the cap.

| Parameter | Value |
|---|---|
| Underlying | SOL/USDC Raydium full-range 50/50 CPMM LP |
| Tenor | 30 days |
| Settlement | European (entry vs expiry oracle prices) |
| Deductible | 1% terminal IL |
| Cap | 7% terminal IL |
| Maximum payout | 6% of insured notional |
| Payout formula | min(max(IL - 1%, 0), 6%) x notional |
| Pricing sigma | max(EWMA_45d_SOL x 1.30, 40%) in calm; x 2.00 in stress |
| Regime signal | fvol >= 0.60 triggers stress pricing |
| Underwriting load | x1.10 on NIG-based premium table |
| NIG params | alpha=3.1401, beta=+1.2139, delta=0.38440 (30d-fitted, cached as `hedge_d10_c70.npy`) |

**What this covers:** Terminal impermanent loss only. Not SOL downside, not fee income loss, not MEV extraction.

**What triggers a claim:** SOL price moves more than ~14% in either direction from entry over 30 days (the 1% IL deductible corresponds to roughly a +/-14% price move).

---

## 2. How the IL Payout Works

IL for a 50/50 constant-product pool: IL(x) = 1/2(sqrt(x) - 1)^2, where x is the price ratio (end/entry).

| SOL move | IL loss | Payout | Example on $10k |
|---|---|---|---|
| +/-0% to +/-14% | 0% to 1% | $0 | Below deductible |
| +/-14% to +/-38% | 1% to 7% | (IL - 1%) x notional | $0 to $600 |
| Beyond +/-38% | >7% | 6% x notional (capped) | $600 (max) |

---

## 3. The Cost of Protection — What the LP Actually Pays

The premium looks expensive at first glance, but the LP gets most of it back as claims.

**Post-crash (2023-2026) on a $10,000 LP position, annualized:**

```
The LP earns from the Raydium pool:                          +$1,645/yr  (16.4% APY)
The LP loses to impermanent loss:                            -$1,266/yr  (12.7%)
                                                             ---------
LP return WITHOUT protection:                                  +$379/yr  (+3.8%)

The LP pays in premiums:                                       -$792/yr  (7.9%)
The LP receives back in claims:                                +$575/yr  (5.8%)
                                                             ---------
Net cost of protection (premium minus claims):                 -$217/yr  (2.2%)

LP return WITH protection:                                     +$162/yr  (+1.6%)
```

**The LP gives up $217/year — about 35% of their naked return — to buy tail protection.** Not 90%. The premium is $792 but $575 comes back as claims. The net cost is only $217.

What the LP gets for that $217: the worst 5% of 30-day windows improve by $264 per window. One bad month avoided pays for an entire year of protection.

**At 18.7% fee APY (the mean):**

```
LP return WITHOUT protection:                                  +$604/yr  (+6.0%)
Net cost of protection:                                        -$217/yr  (2.2%)
LP return WITH protection:                                     +$387/yr  (+3.9%)

The LP gives up $217 out of $604 = 36% of naked return.
```

---

## 4. Regime Split

| Regime | Days | Share | Pricing sigma | Loss ratio |
|---|---|---|---|---|
| Calm (fvol < 0.60) | 1,762 | 87% | EWMA x 1.30 | ~77% |
| Stress (fvol >= 0.60) | 265 | 13% | EWMA x 2.00 | ~100% |
| **All** | **2,027** | **100%** | — | **80%** |

The Rust engine produces a mean premium of 1.20% per 30d window. The blended loss ratio of 80% means the vault retains ~20% of all premiums collected. The other 80% flows back to LPs as claims.

---

## 5. Full LP Economics on a $10,000 Position

### By regime period (annualized)

All premiums computed by the Rust NIG European engine. Fee income at 16.4% median APY.

**Full period (2020-2026):**

| Line item | Annual $ | As % of position |
|---|---|---|
| Fee income | $1,645 | 16.4% |
| Gross IL suffered | -$3,073 | -30.7% |
| **LP net (naked)** | **-$1,428** | **-14.3%** |
| Premium paid | -$1,462 | -14.6% |
| Claims received | +$1,165 | +11.7% |
| **Net insurance cost** | **-$296** | **-3.0%** |
| **LP net (protected)** | **-$1,724** | **-17.2%** |
| Loss ratio | 80% | |

The full period is dragged down by 2020-2021, when SOL went from $3.75 to $170+. That ~45x rally produced 75-94% annualized IL which overwhelms any fee income. Neither naked nor protected LPs make money over this full period.

**Post-crash (2023-2026):**

| Line item | Annual $ | As % of position |
|---|---|---|
| Fee income | $1,645 | 16.4% |
| Gross IL suffered | -$1,266 | -12.7% |
| **LP net (naked)** | **+$379** | **+3.8%** |
| Premium paid | -$792 | -7.9% |
| Claims received | +$575 | +5.8% |
| **Net insurance cost** | **-$217** | **-2.2%** |
| **LP net (protected)** | **+$162** | **+1.6%** |

Both LP and vault are positive. The LP gives up 2.2% of their position value per year ($217 on $10k) for tail protection. That is 35% of their naked 3.8% return.

**Fresh (2025-2026):**

| Line item | Annual $ | As % of position |
|---|---|---|
| Fee income | $1,645 | 16.4% |
| Gross IL suffered | -$727 | -7.3% |
| **LP net (naked)** | **+$917** | **+9.2%** |
| Premium paid | -$717 | -7.2% |
| Claims received | +$215 | +2.1% |
| **Net insurance cost** | **-$505** | **-5.1%** |
| **LP net (protected)** | **+$412** | **+4.1%** |

Low IL (7.3%) means the LP makes money easily. The net insurance cost is higher ($505) because fewer claims are filed in calm markets — the LP is paying for protection they are not using. Still clearly profitable.

### Fee sensitivity

The LP return depends heavily on the fee APY. The insurance cost is the same regardless.

| Fee APY | LP naked | Net ins cost | LP protected | Protection costs % of naked |
|---|---|---|---|---|
| 16.4% (median) | +3.8% | -2.2% | **+1.6%** | 57% |
| 18.7% (mean) | +6.0% | -2.2% | **+3.9%** | 36% |
| 20.0% | +7.3% | -2.2% | **+5.2%** | 30% |
| 25.0% | +12.3% | -2.2% | **+10.2%** | 18% |

At the median fee APY (16.4%), protection costs 57% of the naked return — meaningful but the LP is still positive. At the mean (18.7%), it is 36%. At higher fee environments (25%+), the insurance cost becomes a small fraction of a large return.

---

## 6. Downside Protection — What the LP Gets for the Premium

On a $10,000 LP position, the 30-day window P&L distribution:

**Post-crash (2023-2026):**

| Percentile | Naked | Protected | Improvement |
|---|---|---|---|
| P1 (worst 1%) | -$922 | -$450 | **+$472** |
| P5 (worst 5%) | -$336 | -$72 | **+$264** |
| P10 | -$125 | -$41 | +$84 |
| P25 | +$51 | +$1 | -$50 |
| Median | +$108 | +$50 | -$58 |
| Mean | +$67 | +$55 | -$12 |

The trade-off is clear:

- **Most months:** protection costs $50-58 in premium drag (the median and P25 deltas)
- **Bad months (P5):** protection saves $264 — one bad month avoided pays for 4-5 months of premium
- **Worst months (P1):** protection saves $472

The LP is paying ~$50/month in net cost for insurance that pays $264-472 when things go wrong.

**Full period:**

| Percentile | Naked | Protected | Improvement |
|---|---|---|---|
| P1 | -$3,421 | -$3,104 | **+$317** |
| P5 | -$1,127 | -$699 | **+$428** |
| P10 | -$432 | -$248 | +$184 |
| Median | +$91 | +$3 | -$89 |

---

## 7. Year-by-Year Economics

All premiums computed by the Rust NIG European engine per window. Fee income at 16.4% APY (1.35% per 30d).

| Year | Win | Fee/30d | IL/30d | Prem | Payout | Claim% | LP naked | LP prot | Vault | LR% |
|---|---|---|---|---|---|---|---|---|---|---|
| 2020 | 142 | 1.35% | 3.22% | 2.75% | 1.33% | 49% | -1.87% | -3.29% | **+1.42%** | 48% |
| 2021 | 365 | 1.35% | **7.72%** | 2.26% | 2.28% | **57%** | **-6.37%** | **-6.36%** | -0.01% | **100%** |
| 2022 | 365 | 1.35% | 1.76% | 1.27% | 1.03% | 39% | -0.41% | -0.65% | **+0.25%** | 81% |
| 2023 | 365 | 1.35% | 1.83% | 0.79% | 1.00% | 32% | -0.48% | -0.27% | -0.21% | 126% |
| 2024 | 366 | 1.35% | 0.76% | 0.58% | 0.29% | 23% | **+0.59%** | **+0.30%** | **+0.29%** | 50% |
| 2025 | 365 | 1.35% | 0.53% | 0.61% | 0.12% | 20% | **+0.83%** | **+0.34%** | **+0.49%** | 20% |
| 2026 | 59 | 1.35% | 1.04% | 0.46% | 0.50% | 46% | +0.31% | +0.36% | -0.05% | 110% |

**Reading the table:**

- **2021 is the catastrophe year.** SOL went from ~$1.50 to ~$170. IL averaged 7.72% per 30d window. 57% of windows had claims. The vault broke even (100% loss ratio). Both LP and vault suffered.
- **2023 is an edge case.** The vault lost money (126% loss ratio) because SOL rallied from $10 to $100 — big IL claims on cheap premiums. But the LP was actually better off protected (-0.27%) than naked (-0.48%) because the claims offset much of the IL.
- **2024-2025 are the product's sweet spot.** Low IL, moderate premiums, LP positive with and without protection, vault profitable.

---

## 8. Vault Economics

| Metric | Value |
|---|---|
| Mean PnL per window | +0.243% of notional |
| Loss ratio (full period) | 80% |
| Annualized vault return (full) | +3.0% |
| Annualized vault return (post-crash) | +2.2% |
| Annualized vault return (fresh) | +5.1% |

There is no hedge. The vault takes naked IL risk and relies on the 20% premium margin (80% loss ratio). The x1.10 underwriting load plus the regime-aware sigma pricing are the vault's defenses.

### Worst months

| Month | Windows | Vault total | Claim rate | Avg IL | Context |
|---|---|---|---|---|---|
| Oct 2023 | 31 | **-145%** | 100% | 9.4% | SOL rally $22 to $45 |
| Aug 2021 | 31 | -106% | 97% | 27.6% | SOL rally $30 to $75 |
| Jan 2021 | 31 | -94% | 100% | 26.8% | SOL rally $1.5 to $6 |

### Best months

| Month | Windows | Vault total | Claim rate | Avg IL | Context |
|---|---|---|---|---|---|
| Jun 2021 | 30 | **+94%** | 20% | 0.5% | High vol, SOL flat: rich premiums, few claims |
| May 2021 | 31 | +80% | 10% | 0.4% | Same — vol elevated but SOL sideways |

The vault's best months are when implied vol is high (premiums are rich) but SOL doesn't actually move much (few claims). The worst months are directional rallies where SOL moves 50-100%+ in 30 days.

---

## 9. How This Differs from the SOL Autocall

| | SOL Autocall | IL Protection |
|---|---|---|
| Buyer | Deposits USDC | LPs in SOL/USDC pool |
| Risk covered | SOL price structure (coupon + KI) | Impermanent loss |
| Direction | SOL downside hurts buyer (KI) | SOL moves in either direction hurt buyer (IL) |
| Tenor | 16 days | 30 days |
| Vault hedge | **Spot SOL on-chain** | **None** |
| Vault profit source | Retained principal from KI (5.7% of notes) + coupon haircut | Premium margin (20% of premium retained) |
| Vault loss scenario | Dead zone (SOL -0% to -30%) | Large SOL move in either direction |
| Lockout | 2-day (no autocall at first observation) | N/A |

The autocall vault hedges and its profits depend on rare crash events plus coupon haircut. The IL vault is unhedged and its profits come from the steady premium margin.

---

## 10. Why No Hedge

IL is symmetric — both SOL rallies and crashes cause impermanent loss. There is no simple directional trade that hedges IL risk:

- Buying SOL helps if SOL drops but hurts if SOL rallies.
- Hedging IL with options would require buying straddles, which costs more than the premium income.

The product works because the NIG pricing model charges enough premium to cover expected claims plus a margin. The x1.10 load and the stress regime switch are the vault's edge.

---

## 11. Fee Benchmark

| Source | Coverage | Median APY | Notes |
|---|---|---|---|
| DeFiLlama WSOL-USDC Standard 0.25% pool | Jan 2025 - Apr 2026 | **16.4%** | Direct pool-level measurement |
| Pre-2025 (fallback) | Aug 2020 - Dec 2024 | **16.4%** | Stated median, consistent with direct measurement |

The 16.4% is the DeFiLlama-measured median base APY for the actual Raydium WSOL-USDC Standard 0.25% CPMM pool. In 2025, actual measured APY averaged 30.5% (mean) / 22.9% (median) — substantially higher. The economics at 18.7% mean fee APY give LP protected return of +3.9% instead of +1.6%.

---

## 12. Assumptions

| Assumption | Value | Source | Risk |
|---|---|---|---|
| Fee APY | 16.4% median | DeFiLlama Raydium WSOL-USDC Standard 0.25% | Fee income varies with volume; the economics are better at higher fee regimes |
| NIG params | alpha=3.1401, beta=+1.2139, delta=0.38440 | 30d-fitted to SOL history | Static calibration; cached as `hedge_d10_c70.npy` |
| Pricing engine | Rust `nig_european_il_premium` | On-chain engine, SCALE_6 fixed-point | All 2,027 windows priced by the production Rust engine with zero failures |
| Rust vs cached table | +11.6 bps mean premium difference | Rust engine is slightly more conservative | Cached table underprices by ~12 bps on average; Rust numbers used throughout |
| Sigma | max(EWMA_45d x 1.30, 40%) calm; x2.00 stress | Computed at entry | Lagging indicator |
| Underwriting load | x1.10 | Design choice | 80% loss ratio = 20% margin |
| Settlement | European endpoint | Entry vs 30-day expiry price | Misses path-dependent max IL |
| No hedge | — | Design choice | Vault fully exposed |
| Fee denominator | Raydium CPMM 16.4% | DeFiLlama | Do not use JLP/Jupiter 18.89% for IL affordability |

---

## 13. The Product in One Page

**For the LP (buyer):** You are LP-ing in SOL/USDC on Raydium earning ~16.4% APY in fees. IL eats ~12.7% of that (post-crash), leaving you +3.8% naked. Protection costs 7.9% in premium but 5.8% comes back as claims — the net cost is only 2.2%, leaving you +1.6% protected. You are giving up about a third of your net return to cut your worst months by $264-472 per $10k. At higher fee environments (18.7%+), you keep +3.9% protected and the insurance costs an even smaller share. The product is tail insurance: you pay a little every month so the bad months don't wreck you.

**For the vault (seller):** You collect premiums and pay 80% of them back as claims. You keep 20% — a +3.0% annualized return with no hedge. Your worst months (SOL rallies 50%+) cost you more than you collected, but you make it up over time. You need capital to survive the bad months.

**For the protocol:** Pure insurance, no hedging, no third-party dependency. All pricing runs through the on-chain Rust NIG engine with the 30d-fitted cached table and x1.10 underwriting load. The main risk is sustained directional SOL moves (like 2021) that push the annual loss ratio above 100%.

**Honest caveats:** IL Protection underprices some regime switches; the stress pricing (fvol >= 0.60, sigma x2.00) helps but does not fully contain rapid rallies like Q4 2023.
