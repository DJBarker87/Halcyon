# Halcyon Competitive Landscape: On-Chain Structured Products, Options & IL Protection

**Research Date: April 2026**

---

## Executive Summary

The on-chain structured products space is a graveyard littered with failed protocols. Of ~13 DeFi options vault (DOV) protocols that launched in 2021-2022, **9 have wound down or pivoted** by late 2024. Friktion (Solana), Ribbon Finance (Ethereum), PsyFi/Katana (Solana), and Cega (Solana/ETH) are all dead, dying, or at negligible TVL. The survivors pivoted away from structured products into perps exchanges (Aevo, Zeta/Bullet) or general options (Derive/Lyra, Stryke/Dopex).

**No protocol on any chain currently offers autocallable notes, buffer products, or defined-outcome contracts on-chain. No protocol prices structured products on-chain using fat-tailed models. Halcyon's competitive position is effectively greenfield.**

The nearest competitors are:
- **Drift Vaults** (Solana) -- "structured products" label but actually yield strategy vaults (hedged JLP, basis trades), not defined-outcome products. Currently reeling from a $285M hack (April 1, 2026).
- **SuperHedge** (Ethereum) -- principal-protected notes using Pendle PT + options. Closest conceptual competitor but on ETH, no on-chain pricing engine, early stage.
- **Typus Finance** (Sui) -- DeFi options vaults with dual currency notes. ~$17M TVL. Basic DOV, not structured products in the TradFi sense.

---

## Part 1: Solana-Specific Competitors

### 1. Friktion -- DEAD

| Field | Detail |
|---|---|
| **Chain** | Solana |
| **Status** | Dead -- shut down January 2023 |
| **What they did** | Structured product "Volts": covered calls, put-selling, crab strategy, basis yield, capital protection |
| **Peak TVL** | $150M (April-May 2022) |
| **TVL at shutdown** | 96% off highs (~$6M) |
| **Why they died** | FTX collapse cratered Solana ecosystem; multiple Solana outages; costs outpaced revenue; founder disagreements. Officially cited "challenging economics." |
| **Relevance to Halcyon** | Proves Solana structured products had demand ($150M TVL) but the V1 approach (DOVs selling options to market makers) was fragile. Friktion never priced on-chain -- vaults executed strategies, not defined-outcome contracts. |

Sources: [CoinDesk](https://www.coindesk.com/tech/2023/01/30/defi-project-friktions-shutdown-said-to-stem-partly-from-founder-disagreement), [Structured Retail Products](https://www.structuredretailproducts.com/insights/78787/cryptos-solana-structured-product-provider-friktion-to-close-shop)

---

### 2. PsyOptions / PsyFi -- DEAD

| Field | Detail |
|---|---|
| **Chain** | Solana |
| **Status** | Dead -- PsyFi shut down by May 1, 2025 |
| **What they did** | PsyOptions: vanilla American/European options protocol. PsyFi: options vaults (acquired Katana in April 2023 to consolidate Solana options vault market) |
| **Peak TVL** | Modest (sub-$20M) |
| **Current** | PSY token has near-zero trading volume, market cap under $10M. Operations fully ceased. |
| **Why they died** | Liquidity dried up; Solana options market couldn't sustain dedicated options protocol after FTX collapse. Anonymous team. |
| **Relevance to Halcyon** | PsyFi/Katana were the last active Solana options vault protocols. Their death left a complete vacuum -- no Solana protocol currently offers options vaults or structured products. |

Sources: [The CC Press](https://theccpress.com/psyfi-shutdown-may-2025-defi-impact/), [Solana Ecosystem](https://solana.com/en/ecosystem/psyoptions)

---

### 3. Cega Finance -- EFFECTIVELY DEAD

| Field | Detail |
|---|---|
| **Chain** | Solana, Ethereum, Arbitrum |
| **Status** | Zombie/effectively dead. TVL: $416K (down from $16.6M in June 2024) |
| **What they did** | First protocol to create "exotic options structured products" on-chain. Fixed Coupon Notes (FCNs), barrier options, shark fin vaults. Raised $9.3M total funding. |
| **Peak TVL** | ~$16.6M (June 2024) |
| **Current TVL** | ~$416K |
| **Why they're dying** | Acquired November 2024 (details unclear). TVL in freefall. Products were off-chain priced with on-chain settlement -- not truly verifiable. Used market maker counterparties. 27-day lock-up periods hurt adoption. |
| **Relevance to Halcyon** | Cega is the closest historical comparison. They built "exotic structured products" but priced off-chain. Their failure validates Halcyon's thesis: protocol opacity (off-chain pricing) doesn't build trust. Also proves demand exists -- even $16M TVL at their peak shows interest in structured payoffs. |

Sources: [DefiLlama](https://defillama.com/protocol/cega), [Solana Compass](https://solanacompass.com/projects/cega)

---

### 4. Exotic Markets (EXO) -- ZOMBIE

| Field | Detail |
|---|---|
| **Chain** | Solana |
| **Status** | Technically alive but negligible activity. Market cap ~$248K. |
| **What they did** | Dual Currency Notes (DCNs) on Solana. Deposit SOL, earn yield, receive payout in SOL or USDC depending on price vs strike. |
| **Peak TVL** | Sub-$5M |
| **Current** | Token trading at fractions of a cent. Near-zero activity. Raised $5M in 2022. |
| **Relevance to Halcyon** | Another Solana structured products protocol that failed to gain traction. DCNs are a simpler product than autocalls/buffers. |

Sources: [CoinTelegraph](https://cointelegraph.com/press-releases/exotic-markets-secures-5-m-to-bring-new-structured-products-to-solana-defi), [CoinBase](https://www.coinbase.com/price/solana-exotic-markets-cqkr)

---

### 5. Zeta Markets / Bullet -- PIVOTED AWAY

| Field | Detail |
|---|---|
| **Chain** | Solana (building Bullet L2 Network Extension) |
| **Status** | Live but pivoted from options to perps/L2 infrastructure |
| **What they did** | Originally: undercollateralized options and dated futures on Solana (Zeta DEX). Also Zeta FLEX for customizable tokenized options. |
| **Current** | Pivoted to building "Bullet" -- Solana's first Network Extension (L2) purpose-built for trading. ZEX token evolving into BULLET token. Approaching public testnet as of early 2025. |
| **Relevance to Halcyon** | Zeta proved that standalone options protocols on Solana struggle. They pivoted to infrastructure. Not a competitor for structured products. |

Sources: [Zeta Markets](https://www.zeta.markets/), [Solana Compass](https://solanacompass.com/projects/zeta-markets)

---

### 6. Drift Protocol Vaults -- NOT STRUCTURED PRODUCTS (+ HACKED)

| Field | Detail |
|---|---|
| **Chain** | Solana |
| **Status** | Live but severely impacted by $285M exploit (April 1, 2026) |
| **What they claim** | "The Platform for Structured Products on Solana" -- launched October 2024 with 20+ yield strategies, $170M+ TVL at peak. |
| **What they actually do** | Yield strategy vaults: hedged JLP, asset-specific hJLP, borrow/lend strategies, delta-neutral basis trades. These are NOT structured products in the TradFi sense -- no defined outcomes, no autocalls, no buffers, no barrier options. |
| **Pre-hack TVL** | ~$550M (includes all Drift, not just vaults) |
| **Post-hack TVL** | Under $250M and falling |
| **The hack** | April 1, 2026: $285M drained via governance compromise using "durable nonces" (pre-signed admin transfers). Attributed to DPRK-linked actors (same group as Radiant Capital hack). Largest DeFi exploit of 2026. |
| **Relevance to Halcyon** | Drift co-opted the "structured products" label for yield vaults. This muddies the market but their products are fundamentally different from what Halcyon builds. The hack creates a trust vacuum in Solana DeFi that Halcyon's verifiable on-chain approach could fill. |

Sources: [Drift Trade](https://www.drift.trade/updates/introducing-drift-vaults-the-platform-for-structured-products-on-solana), [Yahoo Finance](https://finance.yahoo.com/markets/crypto/articles/drift-protocol-hit-285m-exploit-074032288.html), [CoinDesk](https://www.coindesk.com/tech/2026/04/07/solana-foundation-unveils-security-overhaul-days-after-usd270-million-drift-exploit)

---

### 7. cushion.trade -- UNKNOWN STATUS

| Field | Detail |
|---|---|
| **Chain** | Solana |
| **Status** | Unknown. Has a Colosseum Arena page but no public product data found. |
| **What they did** | CPPI (Constant Proportion Portfolio Insurance) portfolio rebalancing. Breakout hackathon Honorable Mention. Accepted into Colosseum Accelerator (C2 or later). |
| **Current** | No verifiable TVL, website, or product launch found. May still be in accelerator/stealth mode. |
| **Relevance to Halcyon** | Halcyon's whitepaper explicitly names cushion.trade as the closest Colosseum project -- CPPI rebalancing, not structured product pricing. Different product category. |

Sources: [Colosseum Arena](https://arena.colosseum.org/projects/explore/cushion.trade)

---

### 8. Solana IL Protection Protocols -- NONE EXIST

| Field | Detail |
|---|---|
| **Status** | No dedicated IL protection/insurance protocol exists on Solana as of April 2026. |
| **What exists instead** | Amulet Protocol (AmuShield) offers general DeFi insurance (smart contract risk, depeg risk) -- not IL-specific. Kamino Finance uses automated rebalancing to reduce IL in CLMM vaults. Hedging via perps on Drift/Jupiter is the practical workaround. |
| **Relevance to Halcyon** | Halcyon's IL Protection product has zero direct competition on Solana. The market is wide open. |

Sources: [Solana Compass](https://solanacompass.com/projects/category/defi/insurance)

---

## Part 2: Cross-Chain Competitors

### 9. Ribbon Finance / Aevo -- RIBBON DEAD, AEVO PIVOTED TO PERPS

| Field | Detail |
|---|---|
| **Chain** | Ethereum (Ribbon); Aevo L2 rollup (Aevo) |
| **Status** | Ribbon: permanently shut down December 2025 after $2.7M oracle exploit. Aevo: live as perps/options exchange. |
| **History** | Ribbon was the largest DeFi structured products protocol. Peak TVL $300M. Theta Vaults (covered calls, put-selling). Merged into Aevo (July 2023). |
| **What killed Ribbon** | December 2025: oracle exploit on legacy Ribbon vaults drained ~32% of remaining vault assets ($2.7M). All Ribbon vaults permanently decommissioned. Six-month claims window through June 2026 with up to 19% recovery. |
| **Aevo today** | Derivatives exchange (perps + options orderbook). All-time high TVL over $350M, $10B+ total notional traded. No longer offers structured product vaults. |
| **Relevance to Halcyon** | Ribbon's rise and fall is the cautionary tale. Peak $300M TVL proves massive demand for on-chain structured products. Death by oracle exploit + off-chain pricing model proves the need for verifiable on-chain pricing. Aevo abandoned structured products for derivatives exchange. |

Sources: [Blockchain Magazine](https://blockchainmagazine.com/finance/aevo-exchange-suffers-2-7-million-oracle-exploit-in-legacy-ribbon-vault-attack/), [Nansen](https://research.nansen.ai/articles/ribbon-finance-the-defi-structured-products-protocol)

---

### 10. Derive (formerly Lyra Finance) -- ACTIVE OPTIONS PROTOCOL, NOT STRUCTURED PRODUCTS

| Field | Detail |
|---|---|
| **Chain** | Derive L2 (OP Stack Ethereum rollup) |
| **Status** | Live. ~$58M TVL. DRV token launched January 2025. |
| **What they do** | Decentralized options, perpetuals, and "structured financial products" via AMM. Automated market maker for options pricing (Black-Scholes based). |
| **Products** | Options trading, covered calls, spreads. Not autocalls or defined-outcome products. |
| **Relevance to Halcyon** | Derive is an options exchange, not a structured products issuer. They provide the trading layer, not the product layer. Different market segment. Uses Black-Scholes -- exactly the model Halcyon's whitepaper argues misprices crypto by 40-60%. |

Sources: [DefiLlama](https://defillama.com/protocol/lyra), [tastycrypto](https://www.tastycrypto.com/blog/lyra/)

---

### 11. Stryke (formerly Dopex) -- ACTIVE BUT SMALL

| Field | Detail |
|---|---|
| **Chain** | Arbitrum (+ cross-chain) |
| **Status** | Live. Rebranded from Dopex. DPX + rDPX merged into SYK token. |
| **TVL** | Small (exact figure unclear, estimated sub-$10M based on category) |
| **What they do** | CLAMM (Concentrated Liquidity AMM) options. LP/option mechanics innovation. Single Staking Option Vaults (SSOVs). |
| **Relevance to Halcyon** | Options protocol, not structured products. Fills the hedge/income niche on Arbitrum. Not competing in Halcyon's space. |

Sources: [Stryke Blog](https://blog.stryke.xyz/articles/introducing-stryke-the-future-of-crypto-options), [DefiLlama](https://defillama.com/protocol/stryke)

---

### 12. Panoptic -- EMERGING, DIFFERENT MODEL

| Field | Detail |
|---|---|
| **Chain** | Ethereum (built on Uniswap V3/V4) |
| **Status** | V2 in audit (Code4rena, Dec 2025-Jan 2026). Approaching launch. |
| **What they do** | Perpetual, oracle-free options using Uniswap LP positions as the core primitive. "Panoptions" don't expire and don't have time decay. |
| **Funding** | $4.5M raised. Backed by Uniswap Labs Ventures, Coinbase Ventures, Jane Street. |
| **Relevance to Halcyon** | Innovative options primitive but not structured products. Perpetual options are a different category from autocalls/buffers. No defined outcomes. Ethereum-only. |

Sources: [Panoptic](https://panoptic.xyz/blog/panoptic-january-2026-newsletter), [Code4rena](https://code4rena.com/audits/2025-12-panoptic-next-core)

---

### 13. GammaSwap -- ACTIVE, IL-FOCUSED

| Field | Detail |
|---|---|
| **Chain** | Arbitrum, Base, Ethereum |
| **Status** | Live. Working on V2 for 2025. |
| **What they do** | First on-chain perpetual options protocol enabling users to hedge impermanent loss in AMMs and speculate on token volatility. |
| **Products** | Long gamma (buy IL exposure), short gamma (earn yield), straddles. gBTC and gUSDC yield tokens planned. |
| **Relevance to Halcyon** | GammaSwap addresses IL through perpetual options/volatility trading. Halcyon addresses IL through insurance contracts with defined premiums and payoffs. Different approach -- GammaSwap is for sophisticated traders; Halcyon's IL Protection is for retail LPs wanting simple coverage. Not on Solana. |

Sources: [GammaSwap Docs](https://docs.gammaswap.com/), [Mitosis University](https://university.mitosis.org/gammaswap-bringing-on-chain-perpetual-options-to-defi/)

---

### 14. Typus Finance -- ACTIVE ON SUI

| Field | Detail |
|---|---|
| **Chain** | Sui |
| **Status** | Live. ~$17.5M TVL (December 2024). |
| **What they do** | DeFi Options Vaults (DOVs) with Dutch Auction. European-style, fully collateralized. Also SAFU: principal-protected strategy bridging lending + options. |
| **Products** | Covered calls, puts via DOVs. SAFU = deposit into lending protocol + automated options overlay. |
| **Relevance to Halcyon** | Typus is the most active DOV protocol on a non-EVM chain. SAFU is conceptually similar to principal protection but uses simpler strategies (lending yield + basic options). No fat-tailed pricing, no autocalls, no on-chain computation. On Sui, not Solana. |

Sources: [DefiLlama](https://defillama.com/protocol/typus-finance), [Typus Finance](https://typus.finance/)

---

### 15. SuperHedge -- CLOSEST CONCEPTUAL COMPETITOR

| Field | Detail |
|---|---|
| **Chain** | Ethereum |
| **Status** | Live (early stage). Audited by Halborn. Backed by Outlier Ventures. |
| **What they do** | 100% principal-protected structured notes on-chain. Uses Pendle Finance Principal Tokens (PTs) as the floor + options strategies for upside/downside protection. |
| **Products** | "Liquid Structured Notes" with boosted yield. Depositors get principal protection at maturity through Pendle integration, plus options overlays for enhanced returns. |
| **TVL** | Not disclosed (very early). |
| **Pricing** | Does NOT price on-chain. Uses Pendle PT mechanics for principal protection and external options for the overlay. No proprietary pricing engine. |
| **Relevance to Halcyon** | SuperHedge is the closest competitor by product type -- both offer "structured notes" with downside protection. Key differences: (1) SuperHedge is on Ethereum, not Solana. (2) SuperHedge piggybacks on Pendle for protection -- no novel pricing engine. (3) No autocalls, no buffer products, no IL protection. (4) No on-chain verifiable pricing. Halcyon's pricing engine is the core differentiator. |

Sources: [SuperHedge](https://superhedge.com/), [Outlier Ventures](https://outlierventures.io/portfolio/superhedge/)

---

### 16. Thetanuts Finance -- SMALL MULTI-CHAIN DOV

| Field | Detail |
|---|---|
| **Chain** | Ethereum, Arbitrum, BNB Chain, Polygon, Avalanche, + 6 others (11 chains total) |
| **Status** | Live. V3 launched. ~$909K TVL. |
| **What they do** | Multi-chain DOVs. Sell options (covered calls, puts) to generate yield. V3 adds lending market + Uniswap V3 pools for trading options tokens. |
| **Relevance to Halcyon** | Classic DOV model. Small TVL despite wide chain coverage. Proves that DOVs alone (without novel product design) don't attract significant capital. |

Sources: [DefiLlama](https://defillama.com/protocol/thetanuts-finance), [CoinGecko](https://www.coingecko.com/learn/thetanuts-finance-v3-the-latest-approach-to-decentralized-options)

---

### 17. Premia (Premia Blue) -- OPTIONS EXCHANGE

| Field | Detail |
|---|---|
| **Chain** | Arbitrum |
| **Status** | Live. TVL grew from $2.6M to $7.4M during ARB STIP incentives. |
| **What they do** | Peer-to-pool options trading with customizable strikes and expirations. American-style options. |
| **Relevance to Halcyon** | Options exchange, not structured products. Different category. |

Sources: [Premia Blue](https://www.premia.blue/), [DefiLlama](https://defillama.com/protocol/premia)

---

### 18. Rysk Finance -- OPTIONS ON ARBITRUM

| Field | Detail |
|---|---|
| **Chain** | Arbitrum, Hyperliquid |
| **Status** | Live. ~$11M TVL. Active points program. |
| **What they do** | Upfront yield on crypto via options strategies. "Rysk Beyond" upgrade planned with GMX/Rage Trade integration. |
| **Relevance to Halcyon** | Options yield, not structured products. Different category. |

Sources: [Rysk Finance](https://app.rysk.finance/), [DefiLlama](https://defillama.com/protocol/rysk-finance)

---

### 19. Hegic -- LEGACY OPTIONS, STILL ALIVE

| Field | Detail |
|---|---|
| **Chain** | Ethereum, Arbitrum |
| **Status** | Alive. Added 0DTE options in 2024. Small TVL. |
| **What they do** | One of the earliest options AMMs. Simplified options buying/writing. |
| **Relevance to Halcyon** | Legacy protocol, options only, not structured products. |

---

## Part 3: TradFi-to-Crypto Bridge

### 20. Ondo Finance -- RWA TITAN, NOT STRUCTURED PRODUCTS

| Field | Detail |
|---|---|
| **Chain** | Ethereum, Solana (expanding 2026) |
| **Status** | Live. $2B+ TVL. Largest tokenized Treasury platform. |
| **What they do** | OUSG (tokenized Treasury fund), USDY (tokenized Treasury note for non-US). Ondo Global Markets: tokenized stocks and ETFs. $6.8B cumulative trading volume. |
| **2026 plans** | Launching tokenized US stocks/ETFs on Solana in early 2026. $200M State Street/Galaxy seed for SWEEP fund. |
| **Relevance to Halcyon** | Ondo tokenizes existing securities -- they do NOT create new structured products. No autocalls, no buffers, no defined outcomes. However, Ondo's tokenized assets could become underlyings FOR Halcyon products (e.g., structured notes on tokenized SPY/QQQ via Ondo). Potential partner, not competitor. |

Sources: [Ondo Finance](https://ondo.finance/), [MEXC News](https://www.mexc.com/news/343332)

---

### 21. Backed Finance (xStocks) -- TOKENIZED EQUITIES

| Field | Detail |
|---|---|
| **Chain** | Ethereum, Base, Solana (via Kraken integration, March 2026) |
| **Status** | Live. Acquired by Kraken (March 2026). 55+ tokenized stocks/ETFs. |
| **What they do** | ERC-20 bTokens/xStocks tracking real-world equities held in Swiss custody. MSFT, TSLA, GOOGL, GME, etc. Regulated under Swiss DLT Act. |
| **Relevance to Halcyon** | Like Ondo, Backed tokenizes existing securities. Does NOT create structured products. Their xStocks could serve as underlyings for Halcyon's SPY/QQQ/IWM worst-of autocalls. Potential feed/partner. |

Sources: [Backed Finance](https://backed.fi/), [Backed News](https://backed.fi/news-updates/backed-launches-five-new-tokenized-equities-including-bgoogl-btsla-and-bgme)

---

### 22. Superstate -- INSTITUTIONAL TOKENIZATION

| Field | Detail |
|---|---|
| **Chain** | Ethereum (+ Solana) |
| **Status** | Live. USTB fund: $967M AUM. Invesco taking over fund management (Q2 2026). Raised $82.5M Series B (February 2026). |
| **What they do** | Tokenized investment products. USTB = Short Duration US Government Securities Fund. Digital transfer agent infrastructure. White-label tokenization for Wall Street. |
| **Relevance to Halcyon** | Superstate tokenizes fund shares, not structured products. Their infrastructure could eventually support tokenized structured notes, but they don't build them. Potential partner for distribution. |

Sources: [Superstate](https://superstate.com/), [Fortune](https://fortune.com/2026/03/24/invesco-superstate-ustb/), [Blockhead](https://www.blockhead.co/2026/03/25/invesco-takes-over-management-of-superstates-967m-tokenized-treasury-fund/)

---

### 23. Securitize -- REGULATED TOKENIZATION PLATFORM

| Field | Detail |
|---|---|
| **Chain** | Ethereum (+ others) |
| **Status** | Live. Powers BlackRock's BUIDL fund ($1.9B). NYSE partnership announced. |
| **What they do** | Regulated digital securities platform. Tokenized stocks (launching Q1 2026), bonds, fund shares. "Real, regulated shares" on-chain. |
| **Relevance to Halcyon** | Infrastructure provider, not product creator. Does not build structured products. Could eventually tokenize Halcyon structured notes if they become registered securities. |

Sources: [The Block](https://www.theblock.co/post/382885/securitize-to-launch-stocks-onchain)

---

## Part 4: Dead Projects -- Complete List

| Protocol | Chain | Peak TVL | Died | Cause of Death |
|---|---|---|---|---|
| **Friktion** | Solana | $150M | Jan 2023 | FTX collapse, Solana outages, costs > revenue, founder disputes |
| **PsyFi** (acquired Katana) | Solana | ~$20M | May 2025 | Solana options liquidity never recovered post-FTX |
| **Katana** | Solana | $750M options traded | Apr 2023 (acquired by PsyFi) | Consolidated into PsyFi, then PsyFi died |
| **Ribbon Finance** | Ethereum | $300M | Dec 2025 | Oracle exploit ($2.7M drained). All vaults permanently decommissioned |
| **Cega** | Solana/ETH/ARB | $16.6M | Dying (2025-26) | TVL collapsed to $416K. Acquired Nov 2024. Off-chain pricing model failed to build trust |
| **Exotic Markets** | Solana | Sub-$5M | Zombie (2024+) | Token at fractions of a cent. Near-zero activity |
| **CoreVault** | Multi | Multi-million | 2022 | Team vanished |
| **Opyn** (original) | Ethereum | N/A | Pivoted 2023 | Team became Aevo. Original options protocol abandoned |
| **Alpaca Finance** | BSC | Multi-million | May 2025 | Announced gradual closure of all products |

### Key patterns in why they died:
1. **Off-chain pricing = no trust.** Every dead structured product protocol priced off-chain and posted results. Users couldn't verify anything.
2. **DOV model is flawed.** Selling options to market makers creates a one-sided market where MMs extract value and vault depositors underperform.
3. **Black-Scholes misprices crypto.** Protocols using BS underpriced tail risk, leading to vault losses during volatility events.
4. **Bear markets kill TVL.** Without sticky institutional capital, retail TVL evaporates in downturns.
5. **No product differentiation.** Most DOVs offered identical covered call/put-selling strategies. No moat.
6. **Security.** Oracle exploits (Ribbon), governance compromises (Drift), and smart contract vulnerabilities destroyed trust.

---

## Part 5: Market Context & Sizing

### DeFi Options Market (late 2025)
- **Aggregate DeFi options TVL**: ~$68.8M across 24 protocols
- **Breakdown**: RFQ-based systems 41%, AMM-based 39%, other 20%
- **On-chain options volumes surged 10x YoY in 2025**

### TradFi Structured Products (for comparison)
- **US structured note issuance (2024)**: $149.4B (+46% YoY)
- **Global autocallable notes**: $127-185B annual issuance
- **Defined-outcome ETF AUM**: $78B across 420 funds
- **Defined-outcome ETF projection (2030)**: $334-650B
- **Global structured products outstanding**: $2-3T+

### The gap
DeFi has ~$69M in options TVL. TradFi has $2-3T in structured products outstanding. The entire DeFi options market is 0.003% of TradFi structured products. This is not a mature market that Halcyon needs to win share from -- it is an empty market that needs to be created.

### Solana DeFi context (2026)
- **Solana total DeFi TVL**: ~$8-11B (fluctuating post-Drift hack)
- **Kamino**: $3.5-4B TVL (lending/liquidity)
- **Jupiter Lend**: $1.65B TVL
- **Drift**: $250M TVL (post-hack, down from $550M)
- **No options/structured product protocol with >$1M TVL on Solana**

---

## Part 6: Competitive Positioning Matrix

| Feature | Halcyon | Drift Vaults | SuperHedge | Cega | Typus (Sui) | Ribbon/Aevo |
|---|---|---|---|---|---|---|
| **Chain** | Solana | Solana | Ethereum | Solana/ETH | Sui | Ethereum L2 |
| **Status** | Building | Hacked | Early | Dying | Live | Ribbon dead / Aevo pivoted |
| **Autocallable notes** | Yes | No | No | No | No | No |
| **Buffer products** | Yes | No | No | No | No | No |
| **IL Protection** | Yes | No | No | No | No | No |
| **On-chain pricing** | Yes (NIG/fat-tailed) | No (yield strategies) | No (Pendle PT based) | No (off-chain) | No (Dutch auction) | No (off-chain/MM) |
| **Defined outcomes** | Yes | No | Partial (principal protection) | No | No | No |
| **Verifiable math** | Yes | N/A | No | No | No | No |
| **Fat-tailed model** | NIG distribution | N/A | N/A | Claims exotic but off-chain | N/A | Black-Scholes |
| **Pricing engine** | SolMath (on-chain) | N/A | Pendle PT floor | Off-chain | Off-chain | Off-chain |

---

## Key Takeaways for Halcyon

1. **The field is empty.** No protocol on any chain offers autocallable notes, buffer products, or defined-outcome contracts with on-chain pricing. Halcyon is genuinely first-to-market.

2. **Every predecessor died from the same disease.** Off-chain pricing, Black-Scholes mispricing, DOV model extracting value to MMs, no verifiability. Halcyon's architecture directly addresses each failure mode.

3. **Demand is proven.** Friktion hit $150M TVL. Ribbon hit $300M. Even Cega hit $16M with exotic products. The demand for shaped payoffs exists -- the product model was wrong.

4. **Solana is a vacuum.** After Friktion, PsyFi, Cega, and Exotic Markets all died, and Zeta pivoted to infra, there is literally no options or structured product protocol with meaningful TVL on Solana. Drift Vaults is the only thing using the "structured products" label, and it's (a) not actually structured products and (b) just got hacked for $285M.

5. **IL Protection is wide open everywhere.** No protocol on any chain offers dedicated IL insurance. GammaSwap offers volatility hedging for sophisticated traders. Halcyon's IL Protection for retail LPs has zero competition.

6. **The TradFi bridge is being built by others.** Ondo, Backed, Superstate, and Securitize are bringing real-world assets on-chain. This creates the oracle infrastructure and asset availability that Halcyon needs for equity-linked structured products (SPY/QQQ/IWM autocalls).

7. **Post-Drift-hack Solana trust crisis is an opportunity.** The Solana Foundation launched STRIDE security program. Halcyon's fully on-chain, verifiable pricing model is precisely the transparency the ecosystem needs right now.
