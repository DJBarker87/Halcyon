# Halcyon

**The first autocallable on US equity indices to run on a blockchain.**

**Real quant finance, running inside a single Solana transaction.**

April 2026

---

## TL;DR

Worst-of equity index autocallables are the largest single category of structured products in traditional finance. US investors bought $120 billion of them last year. Until now, none had ever run on a blockchain.

Halcyon's flagship is the first: an autocallable on tokenized SPY, QQQ, and IWM, priced inside the Solana program that issues it. Over 20 years of backtested equity history (through the GFC, COVID, and 2022), buyers earned ~8.4% annualized, the vault earned ~5.2–5.6% on occupied capital, and zero notes caused insolvency across both sparse-entry (210 monthly windows) and daily-issuance (4,400 windows) cadences.

Five things make this possible now, and make it Halcyon's:

1. **SolMath.** The quant library I built and open-sourced on crates.io. Fast accurate transcendentals, Bessel functions, barrier options, implied volatility, and realistic return distributions that actually price extreme moves, all in fixed-point integer arithmetic inside Solana's 1.4M compute-unit budget. No other on-chain library does this.
2. **Pricing on-chain, not just settlement.** Every quote, every coupon, every payout is computed inside the smart contract. Anyone with the same inputs gets the same number. That's the blockchain advantage that previous attempts skipped.
3. **Spot-only hedging.** No perps, no options, no bridges, no cross-protocol vault dependencies. In a post-Drift-hack environment, architectural isolation is first-order product design.
4. **A factory, not a product.** Every structured product uses the same math primitives SolMath provides. Adding a new one is calibration work, not a pricing-engine rebuild. The flagship is proof the factory works.
5. **Backtests are profitable.** Flagship +5.2% vault return across 20 years with zero insolvencies. IL Protection +2.2% to +5.1% depending on regime. SOL Autocall positive in every backtested year. Documented unit economics per product.

---

Halcyon is a solo project. I'm a mathematics teacher and I built SolMath and the three pricing engines in my spare time, around teaching. The work is single-handed but it isn't casual: every component is validated against professional reference libraries. This document is me making the case that the project is worth funding seriously, not as a side experiment, but as the work I intend to take full-time.

---

## 1. The Category That Matters

US structured note issuance hit a record $194 billion in 2024. Autocallables, the single largest product type inside that category, accounted for roughly $120 billion of US issuance in 2025. Globally the autocallable market was around $185 billion in 2024, equity-linked by a large majority. iCapital alone processed $84.5 billion of structured investment sales in a single year, and Calamos now packages the same autocall shape as ETFs (CAIE, CAIQ, CAGE) for retail brokerage accounts. This is mainstream distribution, not a niche.

DeFi has tried bringing structured products on-chain before. Cega launched on Solana in June 2022 with worst-of basket notes on ETH, BTC, and SOL (later ARB), peaked near $50 million in TVL, expanded to Ethereum and Arbitrum, and has since declined. Friktion ran covered-call vaults on Solana and shut down in late 2022. Ribbon scaled options strategies on Ethereum before pivoting to Aevo. Why they didn't sustain is complicated, and I don't claim to know the full answer. But 2022 was a hostile environment for DeFi broadly, and the ecosystem looks meaningfully different now. Tokenized asset volumes on Solana are at all-time highs, institutional capital is flowing in through ETPs, Drift Vaults carries over $127 million in structured-product TVL with real performance fees. A Cega launching today would likely find a warmer reception than the one they had.

Three things changed in the last twelve months:

1. **Reliable US equity oracles.** Pyth publishes SPY, QQQ, IWM, and other US equity prices on Solana with sub-second latency.
2. **Tokenized equity wrappers that settle.** xStocks-class tokens (Backed Finance, Dinari, Ondo) provide on-chain exposure to US equities with real liquidity and genuine settlement, so the vault can hedge on-chain.
3. **On-chain pricing math.** Until SolMath, no library existed to price autocallables inside a smart contract's compute budget. I wrote one.

With those three in place, a US equity autocallable can run end-to-end on a blockchain: pricing, hedging, and settlement all inside the Solana program. Halcyon is the stack that makes it work.

The tokenization market is no longer hypothetical. Per the Blockworks Solana Token Holder Report Q1 2026, tokenized asset volumes on Solana reached a new all-time high of $1.3 billion in Q1 2026, up 164% quarter over quarter, with xStocks issuers alone accounting for 41.5% of that volume. Over the same period, Solana overtook Ethereum in RWA lending deposits ($1.23 billion, up 115% QoQ). The underlying asset class the flagship depends on is the fastest-growing tokenized segment on the chain, and Solana is now the leader in the adjacent RWA lending market.

Halcyon's three products target three different buyer pools. The flagship equity autocall targets the capital that already buys US equity autocallables in TradFi: over $100 billion a year in US issuance, distributed through wealth managers, RIAs, and private banks. IL Protection targets liquidity providers on Solana DEXes who want the fee income but are exposed to impermanent loss they can't easily price: the product sells IL tail insurance as a simple, bounded premium. SOL Autocall targets crypto-native users looking for defined-return alternatives to perps and lending. The factory that prices all three runs on the same engine.

The underwriting vault that sits underneath them is itself a product. Depositors stake USDC and earn the margin that flows across all three notes, producing yield that isn't correlated with lending demand, DEX volume, or staking rewards. The three pools above are the buyers; the vault is how the crypto-native capital that doesn't want to think about payoff shapes participates in the same flow.

---

## 2. SolMath

Any non-trivial financial math needs two things: fast accurate transcendentals (logarithm, exponential, square root) that show up inside every pricing formula, and a realistic model of how prices actually move.

The standard options pricing model is Black-Scholes. It works reasonably well for low-volatility assets where the chance of an extreme move is small. For anything more serious, and especially for crypto, Black-Scholes underprices extreme moves. That isn't a theoretical problem. A vault that prices its products with Black-Scholes is charging too little for the scenarios that actually wipe it out: the crashes, the rallies, the tail events the model says won't happen. Over time those scenarios arrive, the vault pays out more than it collected, and it goes bust. Honest pricing of large moves is the difference between a vault that survives a decade and a vault that survives a bull market.

The industry-standard upgrade is the Normal Inverse Gaussian distribution, or NIG. It matches real return data much better than Black-Scholes. It is also considerably harder to compute: the NIG density requires a modified Bessel function (specifically K_1), which is not something you get from any standard numerical library. Computing K_1 cheaply and accurately is half the engineering.

None of this fits on-chain in any existing library. Traditional quant tools run on servers in 64-bit floating-point with no compute budget to worry about. Solana has no floating-point at all, and gives you 1.4 million compute units per transaction. Those constraints rule out every pre-existing numerical library I evaluated. Writing one that fits was the work.

SolMath is the result. Fast accurate transcendentals (natural logarithm 22x faster than `rust_decimal`, exp 6x faster, sqrt 6x faster, all validated against mpmath at 50-digit precision). Full NIG pricing through a bespoke Bessel K_1 evaluator built from Abramowitz & Stegun series expansions. The bivariate normal CDF in fixed-point, which I believe is a first on any blockchain and which is what you need to price anything involving two correlated assets. Black-Scholes with all five Greeks for the cases where it still applies, European barrier options across all four barrier types, a three-stage implied volatility solver with production-grade convergence, plus supporting infrastructure for option pricing (Heston, SABR, weighted-pool math). All in deterministic fixed-point integer arithmetic, `no_std`, zero dependencies, published on crates.io. Validated against QuantLib, mpmath at 50-digit precision, and scipy across 2.5 million test vectors (including 443,520 barrier-option vectors against QuantLib's AnalyticBarrierEngine): median agreement with QuantLib on the high-precision path is fourteen decimal places.

Why open source. Verifiable on-chain pricing requires auditable math: a closed-source pricing library can't really claim verifiability. And SolMath is useful on its own. Any Solana program can pull it in for Black-Scholes, implied volatility, barrier options, NIG, or pool math without rebuilding those primitives from scratch. Halcyon is one consumer of the library; there's no reason it should be the only one.

SolMath was published to crates.io on 26 March 2026. Within two weeks, at least one independent Solana builder had integrated it and publicly rearchitected their own hackathon project around it. One integration doesn't prove platform adoption, but it does suggest the friction I hoped to reduce is low in practice: one `cargo add`, institutional-grade pricing math, no further work required.

The commercial implication is bigger than verifiability. Once a library exists that prices financial products fairly inside a Solana transaction, adding a new product to the Halcyon shelf is calibration work and issuance-gate tuning, not a rebuild of the pricing engine. The marginal cost of product number four is a small fraction of product number one.

---

## 3. The Products

### 3.1 The Flagship: 18-Month Autocallable on Tokenized SPY/QQQ/IWM

The flagship is an 18-month autocallable note whose coupon and redemption depend on the worst-performing index among tokenized SPY, QQQ, and IWM. Coupons are observed monthly. Autocall is checked quarterly. A 20% knock-in barrier is monitored daily. Missed coupons accumulate in memory and pay on the next good observation. This is the dominant shelf structure used by iCapital, Halo, and the Calamos autocall ETFs, rebuilt on-chain from the pricing engine up.

Why three names. A single-name autocallable on SPY alone pays a lower coupon because SPY is less volatile than a worst-of basket. Layering three correlated names and paying based on the worst performer sells correlation risk to the buyer in exchange for a richer coupon. When the three names move together on the upside, the note autocalls at par with coupons paid. When one name drops sharply while the others hold up, the buyer receives reduced or missed coupons. When the basket sells off jointly through 20% down, the buyer takes a principal loss. That's the shape investors pay $100 billion a year for in TradFi.

Why the monthly-coupon / quarterly-autocall split. TradFi shelf autocalls split the observation schedule so that coupons pay often (monthly income) while autocall only triggers quarterly (preventing the note from ending too quickly). Running both at quarterly cadence, the simpler structure, nearly doubles the vault's worst-case drawdown in backtest. The monthly-quarterly split cuts CVaR by 36% and halves worst drawdown, at the cost of about 30 bps of vault return. That tradeoff is clearly correct.

**Backtest.** Two cadences over 2006–2026 SPY/QQQ/IWM daily data (20 years including the GFC, 2020 crash, 2022 bear market). Monthly-spaced entries (210 windows, 98% issued) and daily-spaced entries (4,400 windows, 98% issued) produce consistent per-note economics. Pricing uses one-factor NIG with Gaussian residuals under **per-quarter recalibration** (77 calibrations on trailing 252 trading days), and the issuance gate rejects fair coupons outside the 50–500 bps/obs safe range:

|  | Monthly (206 notes) | Daily (4,291 notes) |
|---|---|---|
| Issuance rate | 98% | 98% |
| Buyer realized IRR (mean) | +8.35% | +8.49% |
| Annualized quoted coupon | 14.9% | 15.1% |
| Vault return on occupied capital | +5.17% | +5.63% |
| Mean vault P&L per note | +$3.48 | +$3.82 |
| Knock-in trigger rate | 22% | 22% |
| Notes settling at a buyer loss | 13% | 14% |
| Per-note CVaR(5%) | −$5.96 | −$6.13 |
| Worst book drawdown | −$17.8 | −$265.5 (0.84% of $31.7K peak book) |
| Insolvency events | 0 | 0 |

The gap between the ~15% quoted coupon and the ~8.4% realized buyer IRR is the cost of the knock-ins, missed coupons during drawdowns, and the issuance fee. The buyer earns the full coupon when the basket is healthy and takes the downside when it isn't. The tails are real: the one-factor NIG captures joint left-tail co-movement that a Gaussian copula smooths over. Per-quarter recalibration cuts worst drawdown roughly in half versus a static 20-year-fit calibration at the same quote share. Full year-by-year breakdown and hedge economics in Appendix B.

**How the vault earns.** Income comes from four sources: the 35% retained coupon haircut (the vault issues at 65% of fair coupon), a 100 bp issuance fee on each note, reserve yield on the escrowed notional (accrued on idle cash only), and KI residual when a knock-in note settles below par. The ~5.2–5.6% annualized return on occupied capital is net of hedge execution costs. The junior tranche absorbs the worst-case drawdowns without breaching into senior capital — no insolvencies across the 20-year backtest. The vault locks $113.50 per $100 note ($100 principal + $12.50 junior + $1 issuance fee) for the note's life (~145 days average).

**How the vault hedges.** The vault holds two on-chain equity wrappers directly: SPY and QQQ. IWM exposure is projected into the SPY/QQQ pair through a rolling 252-day regression (IWM ≈ 1.14·SPY − 0.01·QQQ, R² around 80%). The reason Halcyon doesn't hedge IWM directly is that no on-chain IWM wrapper currently carries the liquidity needed for production hedging. The projection is the workaround until that liquidity arrives, at which point the flagship adds a direct third leg and the proxy step drops out. The proxy costs around 70 bps of occupied-capital return versus a hypothetical three-leg direct hedge. The residual 20% of IWM's variance that the regression doesn't capture is the main source of hedge noise.

**Pricing engine.** The pricer uses a one-factor NIG construction with Gaussian residuals: a shared fat-tailed factor drives all three names through calibrated loadings, and a small Gaussian residual captures idiosyncratic spread. This gives heavier joint tails than a Gaussian copula on independent NIG marginals, which is what real equity crashes look like. Calibrations are refreshed **per quarter** on the trailing 252 trading days — 77 per-quarter fits covering 2007-06-29 to 2026-04-10 — and the pricer loads the most-recent fit at each issuance. Technical detail in Appendix B.

#### 3.1.1 Verifiability

The flagship's 20% knock-in barrier is monitored daily: the buyer is protected as long as no name closes below 80% on any trading day. The production pricer, for reasons of compute cost, only evaluates the barrier at the six quarterly observation dates. Pricing that naively would understate the knock-in probability, underprice the risk, and bleed the vault over time. The engine corrects for this with a "daily-KI correction" (the difference between the true daily-monitored probability and the quarterly-observed one) added on top of the base quote.

That correction is computed deterministically, using a 3D spectral method built on the Fang-Oosterlee COS framework. The canonical Fang-Oosterlee recursion drops a cross-term that produces around 1% systematic bias under the asymmetric return distributions equity indices actually exhibit; I re-derived the recursion from scratch to include it. The corrected version is validated against 500,000-path Monte Carlo on a 9-cell grid covering the production parameter range, all within 1e-3 absolute accuracy. The correction table serialises to canonical JSON, hashes to a single SHA-256 value committed on-chain, and regenerates bit-identically from the calibration inputs.

In practice, that means everything in the flagship's pricing pipeline is verifiable-at-recomputation. Anyone with the calibration, the code, and the spec can rerun the pricer on any machine and get the exact same number. That's the verifiability claim a blockchain is supposed to support, and it's what the previous DeFi structured-products attempts couldn't deliver.

### 3.2 Engine Demonstration: IL Protection

IL Protection is for Solana liquidity providers who want the fee income from LP-ing but are nervous about impermanent loss and don't necessarily want to price it themselves.

A 30-day contract that protects SOL/USDC liquidity providers on Raydium against terminal impermanent loss beyond a 1% deductible, capped at 7%. Tail insurance for LP positions: most months it costs the LP a small premium, rare months it pays out enough to matter.

The pricer integrates the impermanent loss payoff against a fat-tailed (NIG) distribution of 30-day SOL log returns, using 5-point Gauss-Legendre quadrature across four payoff regions. This engine demonstrates SolMath's handling of analytical quadrature against fat-tailed densities. Quote cost is around 300K compute units, with all 2,027 backtested windows pricing successfully (zero engine failures).

|  |  |
|---|---|
| Backtest coverage | 2,027 rolling 30-day windows, Aug 2020 to Feb 2026 |
| Loss ratio (full period) | 80% (vault retains 20% of premium) |
| Vault annualized return (full period) | +3.0% |
| Vault annualized return (post-2022) | +2.2% |
| Vault annualized return (2025-2026 fresh) | +5.1% |

Post-2022, the LP gives up around $217 per year on a $10,000 position (about a third of their naked 3.8% return) in exchange for cutting the worst 5% of months by $264-$472. The vault has had losing years: 2021 hit 100% loss ratio during SOL's rally from $1.50 to $170, and 2023 hit 126% during the $10 to $100 run. The product survives because surplus years outweigh loss years and the regime-aware sigma multiplier prices stress periods more conservatively.

IL Protection is unhedged by design. Impermanent loss is symmetric (both SOL rallies and crashes cause it), and there is no simple directional trade that hedges the risk cheaply enough. The vault earns from the 20% premium margin and the x1.10 underwriting load. Full backtest including year-by-year LP and vault economics in Appendix C.

### 3.3 Engine Demonstration: SOL Autocall

SOL Autocall is for crypto-native yield seekers looking for defined-return alternatives to perps and lending.

A 16-day defined-yield note on SOL, with 8 observations every 2 days. The buyer earns a coupon each period SOL is above entry, autocalls at par if SOL is above 102.5% at any observation from day 4 onwards, and takes a principal loss if SOL touches 70% of entry and finishes below entry. A 2-day lockout suppresses autocall at the first observation, guaranteeing every note runs at least 4 days.

The engine demonstrates SolMath's handling of backward recursion with compression. Pricing this note honestly requires 8 observation-date backward steps on a barrier-aware state space, which at full grid resolution would need around 40,000 transition matrix evaluations per quote. That's well outside Solana's compute budget. The production pricer uses per-product POD-DEIM (proper orthogonal decomposition with discrete empirical interpolation) with a keeper-updated reduced operator `P_red(σ)` and a 15-dimensional value basis, bringing the full one-transaction quote path on the default fixed product to about 946K CU. A gated Richardson CTMC fallback handles cases where the compressed basis is not applicable. The historical live-operator E11 variant evaluates 12 operator samples on-chain and lands around 1.36M CU, so it is not the production primary path. This is the one engine that genuinely pushes against Solana's compute ceiling, and getting it to fit took most of the mathematical work behind SolMath's speed.

|  |  |
|---|---|
| Backtest coverage | 1,638 issued notes across 2,042 entry windows |
| Issuance rate | 80.2% (model refuses below a 50bp per-observation floor) |
| Buyer mean return per note | +1.65% |
| Notes profitable to buyer | 94% |
| Vault mean P&L per note | +$4.79 per $1,000 note |
| Rolling reinvestment CAGR | 20.7% |
| Vault positive years in backtest | 7 of 7 |

The vault loses money on 47% of individual notes (the dead-zone case: SOL drops 0-30% but doesn't crash through 70%), and makes it back on autocall margin plus the 5.7% of notes that knock in and generate retained principal. The vault was positive in every year of the backtest, including 2022 (SOL crashed 94% from $170 to $10, which generated KI events and was the vault's third-best year). Full product detail in Appendix D.

---

## 4. One Vault, Three Uncorrelated Failure Modes

All three products share a single underwriting vault. Premium income splits 90% to senior depositors, 3% to the junior first-loss tranche, and 7% to protocol treasury. (The junior tranche itself is a capital reserve sized at 12.5% of each policy's notional — founder-seeded, non-withdrawable while policies are active. The 3% is the *income share*; the 12.5% is the *tranche size*.)

The three products' failure modes are structurally uncorrelated:

| Product | Loses money when |
|---|---|
| Flagship equity autocall | Joint equity drawdown breaching the 20% knock-in |
| IL Protection | Large directional SOL move, either direction |
| SOL Autocall | SOL grinds sideways down (the 0-30% dead zone) |

Three uncorrelated failure modes mean the shared vault is exposed to diversified risk rather than a single dominant scenario. The flagship's worst environment (joint equity selloff) coincides with SOL Autocall's best (deep crashes generate retained principal) in most historical regimes. IL Protection's worst periods (directional SOL rallies) don't correlate with either.

Each product has its own backtested vault economics, documented in Appendices B (WorstOf3), C (IL Protection), and D (SOL Autocall). The canonical per-product annualised returns — computed over each product's full backtest history on full-reserve occupied capital — are:

| Product | Backtest window | Vault annualised return | Insolvency events |
|---|---|---|---|
| Flagship WorstOf3 | 2006–2026 (20yr) | +5.17% (monthly cadence) / +5.63% (daily cadence) | 0 |
| IL Protection | Aug 2020–Feb 2026 (5.6yr) | +3.0% full period, +2.2% post-crash, +5.1% fresh | 0 |
| SOL Autocall | Aug 2020–Mar 2026 (5.6yr) | Positive every backtested year, +$6.48 mean per $1,000 note post-lockout | 0 |

These are the numbers to trust for long-run vault economics.

**Joint-profitability check across the 2020-08-12 to 2024-10-03 overlap window** (the period where all three products have data):

| Year | IL (n / $) | SOL Autocall (n / $) | Flagship WorstOf3 (n / $) | Combined $ |
|---|---|---|---|---|
| 2020 (Aug–Dec) | 142 / +$1.95 | 134 / +$684 | 99 / +$274 | +$960 |
| 2021 | 365 / −$0.13 | 312 / +$1,822 | 252 / +$1,527 | +$3,349 |
| 2022 | 365 / +$0.88 | 315 / +$2,532 | 251 / +$1,253 | +$3,786 |
| 2023 | 365 / −$0.75 | 231 / +$1,934 | 250 / +$1,320 | +$3,253 |
| 2024 (Jan–Oct) | 277 / +$1.00 | 215 / +$19 | 191 / +$1,152 | +$1,172 |
| **Total** | **1,514 / +$2.95** | **1,207 / +$6,991** | **1,043 / +$5,527** | **+$12,521** |

**Every year is portfolio-positive, including 2022 when SOL crashed 94% and equities entered the bear market.** That's the diversification claim: three uncorrelated failure modes, no single year of joint loss across the 4-year overlap.

A single "blended portfolio ROC" across these three products is denominator-sensitive and we deliberately don't publish one. Depending on whether you treat escrowed buyer principal as vault capital (SOL Autocall and the flagship both escrow principal separately from the vault's at-risk reserve per CLAUDE.md), the same P&L produces portfolio returns anywhere from ~+12% (full-collateral view, senior-depositor-honest) to ~+85% (vault-at-risk view, junior-tranche-honest). A realistic senior LP depositor's annualised yield sits in the 20–40% band depending on how their deposit is positioned alongside junior capital versus principal escrow. For long-run expected yield on each product's documented reserve convention, read the per-product numbers above.

Pricer provenance: IL uses the cached 30d `hedge_d10_c70.npy` table with ×1.10 load and fvol-gated regime multiplier (×1.30 calm / ×2.00 stress, 40% floor) — production. SOL Autocall uses the Rust `sol_autocall_hedged_batch` post-lockout hedged replay — production. WorstOf3 uses quarterly-recalibrated one-factor NIG with 500 bps/obs fair-coupon ceiling on q=0.65 daily-cadence entries — production.

Each product has an on-chain issuance gate that refuses to write new policies when the pricing model says the trade isn't profitable enough for the vault. Refusal rates in backtest: IL Protection around 0%, SOL Autocall around 20% (mostly during low-volatility regimes where the coupon can't clear the 50bp floor), flagship equity autocall around 1%. Refusals are on-chain events with no hidden overrides.

---

## 5. The Architecture Discipline

Pricing and settlement run on-chain. Calibration runs off-chain. That boundary is deliberate.

Everything that determines what a user pays or receives happens inside the Solana program that issues the quote. Off-chain components produce static parameters, and those parameters are published to on-chain accounts where anyone can inspect them. The flagship's daily-KI correction table is computed deterministically, committed on-chain as a SHA-256 hash, and published alongside the generator code. Anyone can regenerate the table and verify.

| On-chain (verifiable, replayable) | Off-chain (parameters published to on-chain accounts) |
|---|---|
| NIG premium computation (all three products) | NIG α/β calibration (monthly MLE fit) |
| POD-DEIM online solve (SOL Autocall, keeper-fed `P_red`) | POD-DEIM training (SVD, DEIM cell selection) |
| Richardson CTMC fallback | One-factor NIG factor model calibration |
| IL Gauss-Legendre quadrature | Regime classification (fvol signal) |
| Daily-KI correction (hash-committed) | KI correction table (regenerable from calibration) |
| EWMA volatility update | |
| Settlement payout | |
| Issuance gate evaluation | |
| Vault reserve accounting | |

---

## 6. Architectural Isolation

On 1 April 2026, Drift Protocol was drained of $286 million (more than half its TVL) in the second-largest hack in Solana's history. The attack did not exploit Drift's smart contracts, which had passed audits. Attackers spent six months socially engineering Drift's contributors, compromised their devices, and used Solana's durable nonces feature to pre-sign transactions that eventually granted admin control over Drift's vaults. Once in, they deposited a worthless fake token as collateral and withdrew real assets. At least 20 downstream protocols that depended on Drift's liquidity, vaults, or strategies were also exposed.

That incident crystallizes a design question every on-chain structured-products protocol has to answer: where does your counterparty and dependency surface live? Every dependency on another protocol's vault, perp venue, options market, or bridge is a potential entry point for an attack that has nothing to do with your own code.

Halcyon's hedging architecture has been deliberately minimal:

| Product | Hedge | Third-party dependency |
|---|---|---|
| Flagship equity autocall | Spot SPY and QQQ on-chain wrapper tokens (plus IWM projected into those two) | xStocks-class wrapper issuer (one dependency, named) |
| IL Protection | **None.** Unhedged underwriting. | None |
| SOL Autocall | Spot SOL via Solana DEX swaps (Jupiter, Raydium) | DEX execution only, no protocol-level vault or perp dependency |

No perpetuals. No options. No centralized venues. No bridges. No cross-protocol vault dependencies. For the two products that hedge, the hedge is held as spot tokens. For the one that doesn't, the vault takes the risk directly and earns the margin.

There is one real counterparty exposure: the xStocks-class wrapper for the flagship. If the wrapper issuer fails, the flagship's hedge is impaired. Wrapper-basis behavior under stress has not yet been validated against live markets, and I've named this as the single most important external validation required before the flagship goes to mainnet. It is not glossed over.

This isn't architectural purity for its own sake. Every additional dependency is a potential attack vector. In the post-Drift environment, architectural isolation from protocol-layer dependencies is a first-order product design decision, not a marketing point.

---

## 7. Why Solana

Halcyon is on Solana because Solana gives you 1.4 million compute units per transaction, and nothing else does. Ethereum's gas model, after storage costs, maps to roughly 100K-300K simple operations, which isn't enough for any of the pricing methods above. Every other Solana argument (cheap fees, fast slots, composable programs) is real but generic. The compute budget is what makes Halcyon specifically possible. For concrete scale: a Halcyon quote costs between $0.01 and $0.05 at current Solana fees; the same computation on Ethereum at comparable load would cost $5 to $50. Across the 1,638 SOL Autocall notes in the backtest, that's $16 of quote fees versus $82,000.

---

## 8. What's Built and What's Next

Built as of submission:

* **SolMath.** Open-source on crates.io, around 45KB BPF when feature-trimmed. All the primitives in Section 2, validated against QuantLib, mpmath, and scipy on 2.5 million vectors.
* **Flagship equity autocall engine.** One-factor NIG with Gaussian residuals, K=12 projected copula filter, deterministic 3D spectral daily-KI correction (Fang-Oosterlee COS, corrected for asymmetric-distribution bias), proxy hedge controller. 207 backtested notes over 20 years, zero insolvencies.
* **IL Protection engine.** NIG European 5-point Gauss-Legendre pricer plus terminal settlement. 2,027 backtested 30-day windows, zero engine failures.
* **SOL Autocall engine.** Per-product POD-DEIM pricer with keeper-fed reduced operators plus gated Richardson CTMC fallback plus barrier-aligned grid plus Brownian bridge KI correction plus delta surface. The historical E11 live-operator path remains a reference branch, but the shipping fixed-product architecture is the keeper-fed DEIM path recorded in the authority log. 1,638 backtested notes with full hedge replay, positive in every backtested year.
* **Shared underwriting vault.** Capital stack with senior and junior tranches, kinked utilization curve, per-product issuance gates.
* **CLI tools.** `make il-hedge`, `make sol-autocall`, and `make equity-autocall` produce quote diagnostics and replay results from JSON payloads.

Roadmap:

Near-term, testnet deployment with live Pyth feeds and real xStocks-class wrapper integration validates the proxy-hedge basis assumption against live market data. Legal partnership discussions begin with potential regulated issuance partners.

Medium-term, new products join the shelf. The same pricing infrastructure that prices the flagship handles a wide range of structured-product shapes: caps, floors, range accruals, twin-wins, reverse convertibles, principal-protected notes. Adding any of these is calibration work and issuance-gate tuning, not a pricing-engine rebuild. That's the payoff on the SolMath investment.

Longer-term, SolMath becomes a platform the ecosystem builds on. Opening the library up to third-party payoff builders, anyone wanting to issue structured payoffs on Solana, turns Halcyon into more than a three-product issuer. The shape of that (protocol partnerships, SDK, embedded CPI integrations) depends on what other teams decide to build.

---

## 9. The Honest Position

**What Halcyon is.** A protocol that issues structured products on-chain, where the pricing and hedging happens inside the Solana program itself. SolMath is the foundation that makes it possible. The flagship equity autocall and its two supporting products are what I built on it during the hackathon.

**How it makes money.** The shared underwriting vault captures the margin on every product on the shelf. Across the three shipped products, backtested vault returns range from +2.2% to +5.6% annualized on occupied capital, all positive. Protocol treasury takes 7% of premium, the junior first-loss tranche takes 3%, senior depositors get 90% of premium income (the junior tranche's capital is a separate 12.5%-of-notional reserve). Revenue scales with issuance volume and product count. This is a structured-products issuance business, with documented unit economics per product.

**Why this scales.** Every structured product decomposes into the same mathematical primitives: realistic return distributions, barrier monitoring, backward recursion, correlation structure, hedging logic. SolMath ships those primitives inside Solana's compute budget. Adding a new product to Halcyon's shelf is calibration work and issuance-gate tuning, not a rebuild of the pricing stack. The marginal cost of product four is a small fraction of product one. That's the lift.

**Demand exists.** Drift Vaults on Solana carry $127M+ TVL across 20+ structured strategies, with performance fees at 20-30%. The buyers are already here and they pay. Separately, tokenized equity volumes on Solana reached an all-time high of $1.3B in Q1 2026, led by xStocks at 41.5% share: the underlying asset class the flagship depends on is the fastest-growing tokenized segment on Solana. Halcyon adds a different product category (fair-priced autocallables with defined payoff shapes) to the same pool.

**What I haven't solved yet.** Three open problems, each being worked on and each real:

*Legal and securities.* Issuing a US equity autocallable to real buyers is selling securities. That requires partnerships with regulated issuance counsel and distribution partners, and that work has not yet been done. Frontier funding buys time to do it properly rather than shipping something I can't defend.

*Hedge instrument liquidity.* The vault hedges the flagship's index exposure by buying on-chain instruments that track each index. Today the best available choice is xStocks-class wrappers (SPYx, QQQx), and their liquidity is improving but not yet at institutional scale. This is a scaling constraint on how large the flagship's book can grow before the hedge needs to move to a different instrument, not a dependency that blocks the product. The pricing, issuance, and settlement layers reference Pyth equity feeds directly and are unaffected by which instrument the vault uses to hedge.

*Distribution.* Cega had a priced product and crypto-native capital available on Solana in 2022, and demand for nuanced crypto-native payoffs still turned out to be thin. Halcyon's thesis depends on a different buyer pool (TradFi-adjacent capital reaching blockchain-issued equity products) which requires distribution partners I don't yet have. Solving this is the biggest single commercial risk, and the Drift Vaults success on Solana suggests the demand is reachable through the right channels, just not the ones I've built yet.

**What I'm confident about.** The math I shipped works. The products price correctly within documented error bounds. The vault margins are documented across 5.6 years of backtests for the crypto-native products and 20 years for the flagship, with zero insolvencies. The architecture has no perp, options, bridge, or cross-protocol vault dependencies. I believe no other team on any blockchain has the capability to issue what the flagship issues. That's enough to be worth building.

---

## Appendices

**Appendix A.** SolMath function-by-function performance table, full validation matrix, QuantLib agreement detail.

### Appendix B. Flagship Worst-of-3 — overlap-window year-by-year

Factor NIG with quarterly recalibration, q=0.65 daily-cadence entries, 500 bps/obs fair-coupon ceiling. Reserve basis: junior + fee = $13.50 per $100 notional (13.5%); full-collateral basis = $113.50 (113.5%). Full 20-year breakdown, hedge proxy regression diagnostics, correlation structure, and factor-model calibration follow the table below.

| Year | Notes | Vault $ | Vault $/note | Res $-yrs (jr) | ROC (junior) | ROC (full) | AC | KI | Loss | Mean life | Buyer ann. IRR |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 2020 | 99 | +$274 | +$2.77 | 338 | +81.2% | +9.7% | 100% | 0% | 0% | 92d | +19.26% |
| 2021 | 252 | +$1,527 | +$6.06 | 3,031 | +50.4% | +6.0% | 52% | 48% | 48% | 325d | +4.28% |
| 2022 | 251 | +$1,253 | +$4.99 | 3,186 | +39.3% | +4.7% | 61% | 38% | 31% | 343d | +13.52% |
| 2023 | 250 | +$1,320 | +$5.28 | 1,401 | +94.3% | +11.2% | 100% | 0% | 0% | 151d | +12.36% |
| 2024 | 191 | +$1,152 | +$6.03 | 884 | +130.3% | +15.5% | 100% | 2% | 0% | 125d | +12.51% |
| **Total** | **1,043** | **+$5,527** | **+$5.30** | **8,839** | **+62.5%** | **+7.4%** | **79%** | **21%** | **19%** | **229d** | **+11.37%** |

### Appendix C. IL Protection — overlap-window year-by-year

NIG European 5-point Gauss-Legendre pricer, terminal settlement. Reserve basis: $0.06 per $1 notional (6% max payout). Buyer is an LP earning Raydium CPMM fees during the 30-day policy; buyer NET is gross payoff plus accrued LP fees minus premium. Regime split, fee sensitivity, and settlement mechanics follow.

| Year | Notes | Vault $ | Vault $/pol | Res $-yrs | Vault ROC | LP APY mean | Buyer gross $/pol | Buyer NET $/pol | Buyer ann. IRR | Buyer loss rate (net) |
|---|---|---|---|---|---|---|---|---|---|---|
| 2020 | 142 | +$1.95 | +$0.01375 | 0.70 | +278.9% | 21.93% | −$0.0138 | +$0.0043 | +5.20% | 58.5% |
| 2021 | 365 | −$0.13 | −$0.00037 | 1.80 | −7.4% | 13.51% | +$0.0004 | +$0.0115 | +13.96% | 49.0% |
| 2022 | 365 | +$0.88 | +$0.00241 | 1.80 | +48.8% | 7.37% | −$0.0024 | +$0.0036 | +4.43% | 62.7% |
| 2023 | 365 | −$0.75 | −$0.00204 | 1.80 | −41.4% | 9.04% | +$0.0020 | +$0.0095 | +11.53% | 31.5% |
| 2024 | 277 | +$1.00 | +$0.00361 | 1.37 | +73.2% | 9.66% | −$0.0036 | +$0.0043 | +5.27% | 26.7% |
| **Total** | **1,514** | **+$2.95** | **+$0.00195** | **7.47** | **+39.6%** | **11.04%** | **−$0.0020** | **+$0.0071** | **+8.67%** | **44.9%** |

### Appendix D. SOL Autocall — overlap-window year-by-year (post-lockout)

Rust `sol_autocall_hedged_batch` post-lockout hedged replay. Reserve basis: recorded occupancy average $224 per $1,000 notional (22%). 2-day autocall lockout applied. Lockout economics, hedge policy detail, and POD-DEIM training procedure follow.

| Year | Notes | Vault $ | Vault $/note | Res $-yrs | Vault ROC | AC rate | KI rate | Mean life | Buyer $/note | Buyer ann. IRR | Buyer loss rate |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 2020 | 134 | +$684 | +$5.11 | 1,254 | +54.6% | 68% | 13.4% | 8.9d | +0.03% | +1.43% | 13.4% |
| 2021 | 312 | +$1,822 | +$5.84 | 2,072 | +87.9% | 82% | 6.4% | 6.7d | +4.06% | +222.69% | 5.8% |
| 2022 | 315 | +$2,532 | +$8.04 | 1,583 | +160.0% | 61% | 12.4% | 9.4d | −2.01% | −77.74% | 12.4% |
| 2023 | 231 | +$1,934 | +$8.37 | 526 | +367.7% | 77% | 1.3% | 7.4d | +1.63% | +80.01% | 1.3% |
| 2024 | 215 | +$19 | +$0.09 | 351 | +5.4% | 73% | 0.5% | 8.3d | +1.00% | +43.75% | 0.5% |
| **Total** | **1,207** | **+$6,991** | **+$5.79** | **5,786** | **+120.8%** | **72%** | **6.7%** | **8.1d** | **+1.02%** | **+46.04%** | **6.5%** |
