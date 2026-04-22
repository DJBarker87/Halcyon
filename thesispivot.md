# Halcyon

The composable structured-product layer for on-chain RWAs.

## 1. What Halcyon is, and why it matters now

Halcyon makes TradFi structured products composable on Solana — meaning the notes we issue remain usable as collateral, margin, or composable inputs throughout their life, not just at settlement. We issue structured notes — autocallables, principal-protected notes, defined-payoff structures — as SPL tokens with deterministic on-chain marks and a mechanically enforced liquidation exit. That combination makes a structured note something TradFi cannot offer: a yield-earning primitive that stays collateralisable throughout its life.

The flagship is an 18-month worst-of-3 autocallable on tokenised SPY, QQQ, and IWM. The pricing engine runs inside the Solana program that issues the note, and every note has a standing, on-chain, mechanically-enforced buyback price available at any moment. Together these two properties make the note a collateralisable-while-alive primitive: a yield-earning note that a lending protocol can accept as collateral because its mark is a computation, not a promise, and its liquidation exit is a smart-contract call, not a dealer's quote.

Alive here means pre-settlement — the 12-to-24-month window between issuance and the note's final payoff. In TradFi that window is dead capital. On Solana it doesn't have to be.

## 2. The RWA context

Solana's tokenised RWA market has crossed $1.7B in early 2026, up from $873M in December 2025 and effectively zero at the start of 2024. In March 2026 Solana overtook Ethereum in RWA holder count for the first time, with 154,942 holders versus Ethereum's 153,592. Holder growth has compounded at double-digit monthly rates through 2025 and early 2026. BlackRock's BUIDL and Ondo anchor the stablecoin-adjacent base. xStocks — tokenised Tesla, Nvidia, SPY, QQQ — are the fastest-growing segment.

Once you hold that exposure on Solana today, there are two things you can do with it. Hold it, or borrow against it. That's the entire surface area.

In traditional finance, the same underlyings feed a $7 trillion structured-products industry, almost all of it behind private-banking gates. Direct ownership is the bottom of the stack, not the top. The gap isn't hypothetical — it's the natural next layer on a base that's already arriving.

## 3. Why it hasn't come on-chain yet

Two reasons, different in nature, and neither is "pricing had to be on-chain."

**Options face adverse selection. Structured products don't.** This is the first-order reason on-chain derivatives have failed as a category while structured products haven't been tried at scale. Zeta pivoted to perps. PsyOptions is dormant. Friktion is dead. An options protocol market-making against informed traders on a live vol surface loses money faster than it can hedge — pros arb the mispricings. Structured products don't have this problem: the payoff is discrete and fixed at issuance, the buyer cannot construct a portfolio that extracts value from a mispriced surface, and the issuer holds the margin. Cega ran autocallables for years without adverse selection; they failed on distribution, not on pricing. This is a structurally different category from options and it survives on-chain where options don't.

**Distribution, not pricing, has been the blocker.** Cega issued autocallables against crypto underlyings for years using off-chain pricing and signed settlement. The product worked; the buyer pool didn't. Crypto-native yield chasers wanted higher, shorter, simpler payoffs; TradFi capital couldn't get on-chain easily enough to buy the product. That era is ending — the RWA base now has 155k holders on Solana who already chose to be on-chain and already understand TradFi payoff shapes. The buyer pool Cega needed is the buyer pool that's now forming.

The pricing infrastructure is a separate story. An off-chain pricer is sufficient for issuance and settlement — Cega proved this. What an off-chain pricer is not sufficient for is collateralisation, which is where the on-chain version produces something genuinely novel. Section 4 covers this.

## 4. Why on-chain specifically — the two legs of collateralisability

A lender accepting anything as collateral needs two things: a price they can defend at liquidation, and an exit at that price. TradFi notes fail both. Signed off-chain pricers fail the first under adversarial conditions. Halcyon delivers both by construction, and neither leg is achievable with a signed off-chain pricer or an external market-maker.

**Leg one: a mark that survives an adversarial dispute.** A lending protocol accepting a Halcyon note as collateral needs to price it continuously under adversarial conditions — stress events, oracle volatility, liquidation scenarios. They won't trust an issuer's signed off-chain server to produce honest marks during a stress event; the issuer has an obvious conflict of interest. A Pyth-signed mark is closer but doesn't solve the specific problem of liquidation disputes, because if the lender and issuer disagree on the mark at the moment of liquidation, the only way to resolve the disagreement without a trusted arbiter is for both parties to reproduce the computation from the same inputs. Reproducibility from on-chain state is the mechanism; dispute-survival is the property it delivers.

**Leg two: a liquidation exit that cannot be refused.** Every Halcyon note has a standing buyback offer from the issuing vault, enforced by the issuing contract. The buyback price is a deterministic function of on-chain state (detailed in Section 5). The vault is mechanically unable to refuse a valid redemption. A lending protocol holding the note as seized collateral can always close the position, in one transaction, at a price they knew in advance. This is the liquidity property lending protocols actually need — not "a secondary market might emerge" but "the issuer itself will take this off my hands at a known price, now, by construction."

Together these two legs turn collateralisability from a rhetorical claim into a mechanical one. A lending protocol can compute the buyback price at any state of the world, set LTV against it, and be certain the exit is available. The mechanism is not a promise the protocol makes; it's a property of the program.

This is also the sharper answer to Cega. Cega's notes had no on-chain utility beyond holding to settlement — a DeFi-native user couldn't deploy the note as collateral, margin, or a composable input anywhere, because there was no way for an integrating protocol to price or exit the position. Halcyon notes are designed to be posted as collateral while earning their coupon, because both the mark and the exit are mechanically guaranteed. Whether this meaningfully changes the distribution problem remains to be demonstrated, but it is the specific unlock that on-chain pricing plus on-chain liquidity provides, and that a signed off-chain alternative doesn't.

One technical note on the pricing. Pricing a worst-of-3 autocallable on realistic return distributions requires transcendental math — Bessel functions, bivariate normal CDFs, NIG characteristic functions — in deterministic arithmetic that fits inside tight compute budgets. It doesn't fit inside the compute budget of any blockchain currently shipping, and even on Solana, nobody had built the library that made it fit. That's SolMath: fixed-point, no_std, validated against QuantLib to fourteen digits across 2.5M test vectors, published to crates.io and independently picked up and used by another Solana builder within two weeks of publication — a small but real signal that the primitives are useful.

## 5. The buyback mechanism

Every note has a standing buyback offer from the issuing vault, at a deterministic price, available at any point during the note's life. The vault cannot refuse. The price is set by rule.

**The accounting property that makes it sustainable.** The capital for each buyback comes from the buyer's own deposit, held against that specific note. The vault is not using shared tranche capital, retained fees, or a separate liquidity pool — it is retiring its own liability by returning a portion of the collateral that liability was backed by. When a buyback is triggered the vault unwinds the note's hedge, combines the unwound proceeds with the USDC reserve held against the note, pays the buyback price, and cancels the note. The vault cannot run out of buyback capital because the capital is reserved per-note, not shared across notes. The vault's profit on healthy-NAV buybacks comes from the buyer paying an early-exit premium against expected maturity value — the buyer is choosing to surrender the note's autocall optionality and coupon stream early, and the haircut compensates the vault for the optionality surrendered. No TradFi structured product has this property, because no TradFi issuer carves the buyer's deposit into hedge-plus-reserve against a specific note — balance sheets are commingled.

**The price formula.**

```
buyback_price = min(KI_level − 10%, current_NAV − 10%)
```

where `KI_level` is the notional-denominated price at which the 80% knock-in barrier sits (not the 80% barrier percentage itself), and `current_NAV` is the fresh on-chain NAV at the moment of the buyback call.

Pre-KI at a healthy NAV, the buyback price is capped at KI−10%. Post-KI or in stressed states, the price follows NAV down at a 10% haircut. The cap at KI−10% is deliberate: it gives lending protocols a deterministic ceiling on liquidation value in healthy states, which is the conservative direction for setting LTV. Setting LTV against this cap means the lender's exit is always covered regardless of how well the note is doing.

**Illustrative economics.** Assuming a $1000 note split into $500 hedge and $500 USDC reserve (delta-driven in practice), with the buyback price set by the formula above:

| Scenario | Spot move | NAV | Buyback price |
|---|---|---|---|
| Healthy, pre-KI | 0% | $970 | $720 |
| Mild stress, pre-KI | −10% | $880 | $720 |
| Near-KI | −18% | $800 | $700 |
| Post-KI, moderate | −25% | $720 | $620 |
| Post-KI, severe | −40% | $580 | $480 |
| Catastrophic | −60% | $400 | $300 |

The vault's profit on each buyback depends on hedge unwind value plus USDC reserve less buyback price. The mechanism is designed so this difference is positive in expectation across the stress range. Validation status: the existing flagship backtest does not include buyback activity. A full backtest rerun with mechanism active — including stressed hedge-unwind slippage — is in the "what's next" list below. Until it is run, the economic claim is designed rather than proven.

**UI treatment.** The buyback price is surfaced in the interface as Lending Value — the price at which any integrated lending protocol can liquidate the note. Retail holders will rarely interact with it directly; it exists to make the collateralisability property legible. Halcyon has no live lending integrations at launch; the value is exposed on-chain so integration partners can build against a deterministic collateral mark.

**Retail redemption path.** Alongside the instant buyback used by lending protocols, Halcyon offers a 48-hour delayed path for retail holders at a tighter haircut. The delay forecloses oracle-latency arbitrage and most short-horizon private-information advantages, which is what allows the tighter haircut.

**What the mechanism does not claim.** The buyback is not a fair-value exit — holders redeeming voluntarily take a haircut relative to expected maturity payoff, and this is correct by design. It absorbs adverse selection from informed sellers and compensates the vault for early-termination risk. The instant-path buyback also assumes execution risk on hedge unwind in stressed markets; the 10% discount is calibrated to absorb this but in extreme liquidity events could be tight. These are implementation constraints, not design compromises, and are covered in the program spec.

## 6. The buyer

The addressable buyer is a Solana account already holding tokenised RWAs — tokenised equity, treasury funds, or stablecoin yield products — who has chosen to be on-chain rather than to hold those positions at a broker or bank. This population has grown from effectively zero in January 2024 to ~155,000 holders in March 2026, compounding at double-digit monthly rates.

What this buyer has today: hold or lend. What they don't have: any equivalent of the structured-yield layer their TradFi counterparts use. Someone holding $100k of tokenised SPY on Solana has no way to convert that exposure into a defined-coupon, defined-barrier note while keeping the position deployable as collateral. They can post the spot SPY token on Kamino and earn ~4% borrow APY. They cannot earn 12% contingent coupon, with defined downside, and still deploy the position as collateral somewhere else.

Halcyon is aimed at that buyer. Not at TradFi institutions — they have better distribution through their private banks. Not at crypto-native yield chasers — they have Drift Vaults and covered-call products. The target is the RWA-native on-chain holder who already understands TradFi payoff shapes and is looking for the next layer of capital efficiency. There are ~155,000 such accounts today; the subset at wealth tiers that justify a structured-note position is smaller but growing with the base. This is the correct beachhead — small enough that a solo-shipped protocol can serve it at launch, growing fast enough that the market is reaching the category rather than the other way around.

## 7. The flagship

A worst-of-3 autocall on tokenised SPY, QQQ, and IWM. Eighteen-month tenor, monthly coupon observations, quarterly autocall observations, continuous 80% knock-in monitoring. Notes are SPL tokens with the deterministic on-chain NAV and the standing buyback described above.

Three properties make this a good collateral asset:

1. NAV is computable from on-chain state at every stage of the note's life. Pre-KI, post-KI, post-autocall — each state has a deterministic pricing path using the same SolMath pricer that quoted the note at issuance.
2. Downside is bounded and derivable. A stressed NAV — "assume KI at current spot with worst performer tracking to maturity without recovery" — is a conservative, computable lower bound a lending protocol can use as a provable collateral mark. The buyback price is computed from this same family.
3. Capital protection plus autocall optionality give NAV support through most of the note's life. KI does not terminate the note; it removes capital protection but the note continues and can still autocall at par if the underlyings recover. In backtest, 34% of notes trigger KI but 23% of those recover to autocall — only 11% of notes end with a principal loss.

**Backtest results, 20 years (Apr 2006 – Apr 2026) including GFC, COVID, and 2022.** 207 notes on rolling 21-day entry spacing with an 18-month tenor — substantially overlapping, effectively ~13 independent windows. The 207 figure is useful for regime coverage (it spans three major drawdowns) rather than for independent-sample statistics.

| Metric | Value |
|---|---|
| Notes issued | 207 on rolling 21-day entry spacing |
| Buyer realised IRR (mean) | +6.2% annualised on deposited notional |
| Buyer P5 return | −17.4% annualised |
| Quoted coupon | 12.6% annualised |
| Notes settling at a buyer loss | 11% |
| KI trigger rate | 34% (of which 23% recover to autocall) |
| Vault return on occupied capital | +6.3% annualised, net of hedge costs |
| Worst vault drawdown | −$10.8 on $100 notional (2008–09) |
| Insolvency events | 0 — absorbed by 12.5% junior first-loss tranche as designed |

The P5 of −17.4% is the left tail the coupon is priced against — capital protection bounds loss and autocall optionality supports NAV through most of the note's life, but the tail is the scenario where both fail and the buyer realises a material loss. This is the outcome the 12.6% quoted coupon compensates for; it is the risk the buyer is being paid to take, not a defect.

Buyer IRR and vault return are close numerically but measured on different capital bases: buyer IRR is on deposited notional, vault return is on occupied capital net of tranche allocations and hedge costs. Both can earn ~6% on their respective bases because the capital structure is layered.

The existing backtest models issuance and settlement; it does not model buyback activity. A mechanism-active backtest is in the "what's next" list.

**Pricing.** One-factor NIG model with Gaussian residuals, calibrated to 20 years of SPY/QQQ/IWM daily data. A shared fat-tailed factor drives all three names through calibrated loadings; a small Gaussian residual captures idiosyncratic spread. Joint tails are heavier than a Gaussian copula on independent NIG marginals, which is what real equity crashes look like.

**Verifiability.** The 20% knock-in barrier is monitored daily but priced at quarterly observations for compute reasons. The gap — the daily-KI correction — is computed deterministically via a 3D spectral method built on Fang-Oosterlee COS, validated against 500,000-path Monte Carlo across the production parameter range, committed on-chain as a SHA-256 hash, and regeneratable bit-identically from the calibration inputs.

**Hedging.** Spot SPY and QQQ on-chain wrapper tokens; IWM projected via rolling 252-day regression (R² ≈ 80%). The proxy costs ~70 bps versus hypothetical direct hedging but avoids a third-wrapper dependency. One named counterparty exposure: the xStocks-class wrapper issuer.

## 8. The platform

One pricing engine, multiple products. Adding a product is calibration and issuance-gate tuning, not a pricing-engine rebuild. The buyback mechanism applies uniformly across the product shelf.

The flagship is the lead. The platform-proof product is SOL Autocall — a crypto-native autocallable on spot SOL, priced via reduced-order methods, 1,638 backtested notes positive in every year, with no wrapper dependency and no regulatory path required. It ships without any of the external counterparty risk the flagship carries.

The same engine also prices IL protection for LPs and other defined-payoff structures — documented in the product shelf, demoted here because the flagship and SOL Autocall carry the pitch.

**Architectural isolation.** No perps, no options venues, no bridges, no cross-protocol vault dependencies. In a post-Drift environment, minimising counterparty surface is a first-order design property, not a marketing line.

## 9. What's built, what's next

**Built.** SolMath on crates.io, adopted, validated to 14 digits against QuantLib. Three pricing engines (flagship, SOL Autocall, IL), backtested, producing positive vault returns across test windows. Anchor programs deployed on devnet with end-to-end quote-to-settlement flow working. WASM build of SolMath for frontend pricing working. All of this built solo in roughly a month.

**Next, within the hackathon window.**
- Buyback mechanism: instant liquidation path and 48-hour delayed retail path, both on-chain.
- Reference liquidation consumer contract demonstrating a third-party program calling the buyback against a posted note.
- Mechanism-active backtest covering stressed hedge-unwind slippage.
- Front-end surfacing of lending value alongside NAV and expected maturity payoff.

**Beyond the hackathon.** Mainnet deployment, legal partnerships for regulated issuance of the flagship, integration with specific lending markets (Kamino, MarginFi, Jupiter Lend) as collateral counterparties. Distribution and regulatory paths, not technical ones.

## 10. What a sharp reader should push back on, and the honest answers

**"On-chain pricing isn't necessary; a signed off-chain pricer would do."**
True for issuance and settlement, not true for collateralisation. A lending protocol accepting the note as collateral cannot trust the issuer's signer to produce honest marks in a stress event, and cannot resolve liquidation-moment mark disputes with a signed oracle without a trusted arbiter. Reproducibility of the computation from on-chain inputs is the specific property that handles both cases.

**"No lending protocol has actually integrated this."**
Correct. The mechanism is demonstrated; integration is distribution work. A three-week hackathon cannot deliver Kamino integration; it delivers a working reference consumer that proves the plumbing and a mechanically-enforced buyback that removes the liquidity objection from the technical list.

**"Trustworthy marks don't matter if there's no liquidity to exit into."**
Sharpest version of the previous pushback. The buyback mechanism is the specific answer: deterministic price, mechanically-enforced, self-funded from the note's own deposit, available in every market state. The lending protocol's exit is guaranteed by the issuing contract rather than by an emergent secondary market. Whether a lending protocol adopts this is a business-development question, not a mechanism question.

**"The buyback concentrates hedge unwinds at stress moments; your daily-liquidity assumptions may not hold."**
Acknowledged. The current backtest assumes $50M daily liquidity via a sqrt-impact function; buyback activity concentrates unwinds at stress moments which may violate this. The mechanism-active backtest with stressed unwind slippage is the next validation milestone and will price this in explicitly rather than assuming it away.

**"The flagship needs regulated issuance to reach US persons at scale."**
Correct. Largest commercial gap. The flagship is architecturally ready and pricing-ready but not securities-ready. Mainnet launch requires a regulated issuance partner. Gated behind distribution partnerships the project does not yet have.

**"You're claiming $7T TAM for a devnet protocol."**
The $7T is the TradFi reference class, not the claimed capture. The honest claim is: if structured products arrive on-chain at any meaningful fraction of TradFi penetration, Halcyon is ahead on pricing infrastructure and composability mechanism. The scale of arrival is unknowable and not Halcyon's to predict.

**"Zero insolvencies in 20 years sounds too good."**
It isn't risk-free underwriting. The 12.5% junior first-loss tranche absorbs all historical drawdowns; the worst drawdown was −$10.8 on $100 notional in 2008–09, comfortably inside the tranche. This is the capital stack working as designed rather than a claim that the product can't lose money.

**"The SolMath adoption signal is one solo dev's hackathon project."**
Correct. Useful because it's independent — a different builder evaluated SolMath and chose it — not because it's institutional validation. Weighted accordingly.

**"Post-KI, the note can still lose more. Your floor isn't a floor."**
The claim is not an absolute floor. The claim is a deterministic, computable lower bound under a worst-case stress path, provably reproducible at every stage of the note's life. Lending protocols don't need an absolute floor — they need a conservative computable mark for setting LTV. That is what this provides.

## Technical notes

**Oracle latency.** The buyback instruction pulls fresh Pyth prices in-transaction rather than reading a keeper-cached NAV. Within Pyth's publish latency the attack surface is bounded but non-zero; the 10% discount is sized to absorb this alongside other adverse-selection costs. Using a stale keeper-cached NAV would create latency arbitrage and is specifically excluded by the instruction design.
