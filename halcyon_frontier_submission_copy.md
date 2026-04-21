# Halcyon — Frontier Submission Copy

Draft modular pieces. Slot into whichever submission fields apply. Numbers checked against project folder.

---

## One-liner

Halcyon brings exotic structured products on-chain. The kind that move hundreds of billions in TradFi, with pricing that runs inside a single Solana transaction.

---

## Tweet-length summary

The first on-chain worst-of autocall on tokenised US equities. Plus IL insurance for Solana LPs and a SOL yield note, all priced by the same Rust engine, all running deterministically inside Solana's compute budget. Math nobody had fit on-chain before.

---

## Short description (~100 words)

Halcyon is a structured products protocol on Solana. The flagship is a worst-of autocall on tokenised SPY, QQQ and IWM. This equity structured note category does $120bn a year in US TradFi but has never run on-chain before, because nobody had fit the pricing math inside a smart contract's compute budget. Two more products demonstrate the same engine at work: IL insurance for Raydium LPs, and a SOL yield note. All three pricers run deterministically in Rust, validated against QuantLib, with quotes and settlements computed inside the issuing transaction. Verifiable pricing, not oracle-posted prices.

---

## Long description (~500 words)

Halcyon is a structured products protocol on Solana, built around a pricing engine that lets exotic derivatives compute their fair value inside a single transaction.

The flagship is a worst-of autocall on tokenised SPY, QQQ and IWM. In TradFi this is a $120bn-a-year US issuance category, the largest single shape of structured note retail buys for yield. It has never run on-chain before, for two reasons. Until recently there were no production-quality tokenised US equity wrappers (xStocks changed that in 2025). And nobody had done the numerical analysis work to fit proper exotic pricing inside Solana's 1.4 million compute unit budget.

That second piece is what we built. The pricing engine handles the modified Bessel functions, fixed-point transcendentals, and spectral methods needed to price multi-asset payoffs with realistic fat-tailed return distributions, all in deterministic integer arithmetic. The high-precision path agrees with QuantLib to fourteen decimal places. The flagship's daily-knock-in correction uses a 3D spectral method built on a corrected Fang-Oosterlee recursion, validated against 500,000-path Monte Carlo and committed on-chain as a SHA-256-hashed table.

Two further products demonstrate that the engine generalises beyond autocalls. IL Insurance protects Raydium SOL/USDC liquidity providers against impermanent loss using analytical quadrature against a fat-tailed density. A SOL yield note offers defined-outcome exposure to SOL with backward-recursion pricing compressed via POD-DEIM into roughly 350K compute units. Different mathematical regimes, same engine. SolMath, the underlying numerical library, is published on crates.io and has already been adopted by another Solana options team as a production dependency.

Twenty years of equity backtests show the flagship returning 6.2% mean realised IRR to buyers (median around 9%, with 89% of notes returning principal) and 6.3% annualised on occupied capital to the vault, with zero insolvency events across the GFC, COVID and the 2022 bear market. The vault hedges directly with on-chain SPY and QQQ wrappers, with IWM exposure projected through a rolling regression. No perps, no cross-protocol collateral dependencies, no shared hedge venue with another protocol.

Drift Vaults proved the demand for sophisticated structured products on Solana, growing to $550M in TVL before the April 2026 hack and holding around $250M after. The current category is dominated by hedged-JLP and borrow-lend strategies, the simple end of structured products. Halcyon is the layer that prices what comes next: multi-asset, path-dependent, fat-tailed. The commercial path forward is open. Operating the protocol, licensing the engine to established Solana derivatives platforms, or both. We're using Frontier to find out which.

Built solo by a mathematics teacher with an Oxford maths background, in spare time around teaching. SolMath is open source. The protocol is in late testnet. The Frontier submission demonstrates the engine end-to-end on mainnet via the IL Insurance product, with a live transaction insuring an actual LP position.

---

## What's the demo

The live mainnet demo opens with a wallet connection that detects an existing Raydium SOL/USDC LP position and quotes IL insurance for it directly. One transaction prices the policy via the on-chain Rust engine, escrows the premium, and issues the policy. The entire pricing pipeline runs inside a single Solana transaction with verifiable on-chain math. The quote can be reproduced bit-identically by anyone with the same inputs. Beyond the demo, the flagship worst-of equity autocall and the SOL yield note are testnet-deployed with full backtests in the submission materials.

---

## How does this make money

Three honest paths, in descending order of near-term certainty. One, vault margin from operating the protocol directly. Coupon haircut, issuer margin, hedge P&L, modelled at roughly $3M annual gross at $50M TVL across the three products. Two, licensing the engine to established Solana derivatives platforms whose current product sets are at the simple end of structured products and want to move into exotic payoffs without rebuilding the numerical infrastructure. SolMath has already been adopted as a dependency by another Solana options team without any commercial outreach. Three, integration consulting for partners building bespoke structured products on the engine. The commercial weighting between these paths is exactly what we'd use accelerator runway to determine.

---

## Why now

Three things changed in the last twelve months that make this category possible on Solana for the first time. Pyth started publishing reliable sub-second US equity prices on-chain. xStocks reached production scale in tokenised US equities, with $25B in transaction volume in eight months. And the Solana RWA stack overtook Ethereum's in lending TVL. The infrastructure to issue, price, and hedge tokenised-equity structured products on-chain didn't exist a year ago. The pricing engine, the piece nobody had built, is what we did.

---

## What's the moat

The numerical analysis work is the moat. Fitting fat-tailed exotic derivatives pricing into Solana's compute budget required novel work in fixed-point transcendentals, modified Bessel function approximations, spectral methods, and reduced-order modelling. Months of work each, validated against QuantLib and Monte Carlo at production tolerances. Numerix and other TradFi pricing vendors won't touch Solana for years; crypto-native teams overwhelmingly lack the numerical depth, which is why existing on-chain options protocols ship with crude pricing. The intersection of "Solana-native Rust" and "production-grade exotic pricing" is narrow, and the head start on building the primitive matters as the category matures.
