# Halcyon — Idea Context

## Product
Structured products engine for Solana. On-chain pricing of worst-of equity autocallables (SPY/QQQ/IWM), IL protection, and SOL autocallable notes using NIG fat-tailed models in fixed-point arithmetic.

## Tagline
Quant math that fits on-chain.

## Target User
TradFi-adjacent capital buying equity autocallables (flagship SPY/QQQ/IWM), DeFi LPs (IL Protection), SOL bulls wanting structured yield (Autocall). Secondary: vault depositors seeking underwriting yield.

## Value Proposition
Verifiable on-chain pricing of structured products. Every quote is a Solana transaction. Settlement uses the same code. No off-chain pricing oracle.

## Landscape

### direct_competitors
- { name: "None", status: "N/A", notes: "No protocol on any chain offers autocallable notes, buffer products, or defined-outcome contracts with on-chain pricing" }

### nearest_adjacents
- { name: "Drift Vaults", chain: "Solana", status: "Hacked ($285M, Apr 2026)", strength: "Brand recognition, existing TVL", weakness: "Not structured products (yield vaults), just got hacked", threat: "Low" }
- { name: "SuperHedge", chain: "Ethereum", status: "Early stage", strength: "Closest product concept (principal-protected notes)", weakness: "No pricing engine, piggybacks Pendle PTs, Ethereum only", threat: "Low" }
- { name: "Typus Finance", chain: "Sui", status: "Live, ~$17.5M TVL", strength: "Active DOV with SAFU principal protection", weakness: "Basic covered calls, no autocalls/buffers, Sui only", threat: "Low" }
- { name: "cushion.trade", chain: "Solana", status: "Unknown (C2 accelerator)", strength: "Colosseum HM, accelerator backing", weakness: "CPPI rebalancing not structured products, no on-chain pricing", threat: "Low" }

### dead_projects
- { name: "Friktion", chain: "Solana", peak_tvl: "$150M", died: "Jan 2023", cause: "FTX collapse, costs > revenue, founder disputes" }
- { name: "Ribbon Finance", chain: "Ethereum", peak_tvl: "$300M", died: "Dec 2025", cause: "Oracle exploit ($2.7M), all vaults decommissioned" }
- { name: "PsyFi", chain: "Solana", peak_tvl: "~$20M", died: "May 2025", cause: "Solana options liquidity never recovered" }
- { name: "Cega", chain: "Solana/ETH", peak_tvl: "$16.6M", died: "Dying 2025-26", cause: "TVL collapsed to $416K, off-chain pricing failed to build trust" }
- { name: "Exotic Markets", chain: "Solana", peak_tvl: "<$5M", died: "Zombie", cause: "Near-zero activity" }

### crowdedness
"empty" — No direct competitors. The entire on-chain structured products category is a graveyard of dead protocols. Halcyon is first-to-market with verifiable on-chain pricing.

### moat_type
"Technical Complexity" — NIG density evaluation, Bessel functions, POD-DEIM reduced-order models in fixed-point arithmetic. Estimated replication time: 6-12 months for a strong quant team. Secondary moat: "Brand/Trust" via verifiable on-chain pricing in a post-Drift-hack trust environment.

### differentiation
Every predecessor died from the same disease: off-chain pricing, Black-Scholes mispricing of crypto tails, DOV model favoring market makers. Halcyon addresses all three: on-chain NIG pricing (verifiable), fat-tailed models (honest), direct underwriting vault (no MM extraction).

### copilot_landscape
Source: Colosseum Copilot search across 5,428 hackathon projects / 293 winners (queried 2026-04-17).

#### similar_projects (no project exceeds 8.3% similarity — genuinely novel positioning)
- { name: "cushion.trade", hackathon: "Breakout 2025", prize: "Honorable Mention $5K", similarity: 0.050, note: "Closest winner analog — uses CPPI + RL (dynamic allocation), not derivatives pricing. Far shallower tech than Halcyon. (Prior 'Trenches.Top C2 accelerator' note in this file was unverified — Copilot only confirms the HM prize.)" }
- { name: "Exponent", hackathon: "Renaissance 2024", prize: "5th Place $5K (verified)", similarity: 0.076, note: "Yield derivatives protocol. Fixed-rate + yield tokenization, not options/autocalls. Verified upper comparable for deep-math DeFi placement." }
- { name: "optmachine", hackathon: "Cypherpunk 2025", prize: "none", similarity: 0.058, note: "Covered-call minting + time-limited AMM. Narrow single-product, no pricing engine." }
- { name: "sBread Market", hackathon: "Renaissance 2024", prize: "none", similarity: 0.056, note: "Delta-neutral Iron Condor vault. Fixed strategy, no pricing primitives." }
- { name: "Vega", hackathon: "Cypherpunk 2025", prize: "none", similarity: 0.053, note: "Solo builder, 'Volatility-Adaptive AMM' on-chain options. No library/validation. **The closest cautionary precedent for Halcyon: solo + ambitious pricing math + no UI = zero prize.**" }
- { name: "Milopt / OpSwap", hackathon: "Cypherpunk 2025", prize: "none", note: "Two more on-chain options protocols in the same Cypherpunk cohort as Vega. Category was contested and none of the four entries placed." }
- { name: "ShieldHedge / LP Agent / Toby / Balancex / Noil", note: "IL-hedging via automation/AI (up to 8.3% sim). **Six projects, zero prizes** — IL Protection is the most crowded and lowest-converting of Halcyon's three angles." }
- { name: "Autonom (REMOVED)", note: "Earlier version of this file claimed 'Autonom — Cypherpunk 2025, 1st Place RWAs $25K'. Re-queried Copilot 2026-04-17: the project is not in the search index and no RWA-oracle project in the closest matches has any prize. Removed as unverifiable." }

#### gap_analysis (winners vs full field, 293 vs 5,428)
- Overindexed by winners (do MORE of):
  - oracle primitive: +27% lift (Halcyon uses Pyth heavily ✓)
  - tokenization: +27% lift (V2 xStocks/Backed ✓)
  - capital inefficiency problem framing: +81% lift ✓
  - liquidity fragmentation: +106% lift (tangential)
- Underindexed by winners (AVOID):
  - NFT primitive: −66% lift (Halcyon: none ✓)
  - gamification: −57% lift (Halcyon: none ✓)
  - tokenized rewards: 0% among winners (Halcyon: none ✓)
  - "high barrier to entry" problem: 0% among winners — caution: don't pitch Halcyon as accessibility
  - "high platform fees" problem: 0% among winners — don't lead with costs

#### crowdedness
Cluster-level: Halcyon's natural cluster is **v1-c8 Solana Yield & DeFi Optimization** (257 projects, 29 winners, ~11.3% win rate — about 2x the field's 5.4% baseline). The adjacent **v1-c9 DEX & Trading Infrastructure** has 323 projects.
Sub-niche level:
- *Fair-priced barrier derivatives on-chain* — uncontested. No other Copilot project has the NIG/Bessel/POD-DEIM stack.
- *On-chain options protocols generally* — contested (Vega, Milopt, OpSwap, optmachine in Cypherpunk 2025 alone, none placed).
- *IL protection* — very crowded, zero winners across 6+ entries.
- *Tokenized-equity yield* — newly crowded after Cypherpunk 2025 (PiggyBank, Shift Stocks, xVaultFi, Stock Stake), no winners yet.

#### winner_pattern_alignment
Halcyon hits 6/6 positive winner signals from the 293-winner gap analysis: oracle (+27% lift), tokenization (+27%), capital-inefficiency framing (+81%), liquidity-fragmentation (+106%), no NFT (winners under-index −66%), no gamification (−57%). Hits 0/4 negative signals (no token-gating, no smart-contract-as-product framing, no "high platform fees" narrative, no "high barrier to entry" pitch). Do NOT pitch Halcyon as "simpler for retail" — that framing has 0% winner share.

#### prize_outlook
The current Colosseum hackathon is **non-tracked** — there is no DeFi / Infrastructure / RWAs split. Halcyon competes against the entire field for a single prize ladder. This *raises* the bar (no track-specific HM safety net) and *changes the strategy*: the pitch must read to a generalist judge, not a DeFi specialist who already knows what an autocallable is. Realistic outcome bracket, anchored to verified comparables:

| Outcome | Anchor | Likelihood |
|---|---|---|
| No prize | Vega (solo, ambitious, no UI → 0) | ~40% |
| Honorable Mention / lower placement | cushion.trade (HM $5k) | ~40% |
| Mid-ladder placement | Exponent (5th place $5k, verified) | ~15% |
| Top-3 / Grand Prize | no clear precedent for solo + math + no-UI | ~5% |

#### judging_risk
Two compounding risks: (1) **No UI** — CLI tools (`make il-hedge`, etc.) do not translate to a judge's 10-minute review. (2) **Generalist audience** — without tracks, the judge pool is broader and less likely to know what NIG, Fang-Oosterlee, or POD-DEIM are. Highest-leverage upgrade before submission: a clickable quote → replay → settlement flow a judge can verify in 60 seconds, with the pitch leading on the *outcome* ("~15% coupon on tokenized SPY/QQQ/IWM") and the math hidden behind a "how it works" link. Vega lost a Cypherpunk slot for exactly this reason.

#### research_sources
Superteam "What to Build for Solana DeFi" names on-chain options as a key gap (original context memo). Colosseum winner distribution confirms: structured-product winners exist (cushion.trade HM, Exponent 5th place) but none have Halcyon's pricing depth.

#### file_consistency_warnings
- `submission/halcyon-submission.html` (Section 3, lines 78–99) still contains a "Track Selection" recommendation block listing DeFi as primary and Infrastructure as secondary. This is obsolete now that the hackathon is non-tracked — remove or rewrite that section before submission.
