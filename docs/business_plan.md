# Halcyon Business Plan

Last updated: 2026-04-20

## 1. Executive Summary

Halcyon is building the issuance and pricing rail for on-chain defined-outcome products on Solana. The core asset is not a single note product. It is a reusable stack that lets partners quote, issue, verify, and settle structured payoffs on-chain using deterministic Rust pricing logic instead of opaque off-chain market-maker quotes.

The commercial strategy is infrastructure first, not direct-to-retail first.

Halcyon's strongest company narrative is the SPY/QQQ/IWM worst-of-3 autocall on tokenized equities. It ties Halcyon to a real TradFi category, a live on-chain RWA market, and a much larger long-term revenue pool than a crypto-only note product. The strongest live demo, however, is IL Protection: connect a wallet, detect a Raydium SOL/USDC LP, and buy matching synthetic cover. SOL note products remain useful as crypto-native bridge products, but they should not anchor the company narrative.

The target business is B2B and B2B2C:

- wallets and yield apps that want differentiated earn products
- tokenized-asset platforms that want note wrappers around existing assets
- DAOs, treasuries, and structured-product distributors that want auditable issuance infrastructure

Halcyon monetizes through integration fees, platform fees on notional issued, servicing and monitoring fees, and optionally underwriting economics where Halcyon or its partners provide the vault capital.

The short version:

- Hackathon story: Halcyon proves complex protection and structured-product pricing can run on-chain.
- Demo story: Halcyon can detect your LP and let you insure it in minutes.
- Company story: Halcyon becomes the trusted issuance rail for tokenized-equity and other defined-outcome products.

## 2. Company Vision

### Mission

Make structured on-chain products auditable, programmable, and distributable by turning pricing and settlement into open, verifiable infrastructure.

### Vision

Halcyon becomes the default backend for on-chain defined-outcome issuance on Solana, similar to how Stripe abstracted payments infrastructure and how Plaid abstracted bank connectivity. Partners should be able to offer transparent note products without standing up a quant desk, a derivatives middle office, and a custom settlement stack.

### What Halcyon Is

- a pricing kernel
- an issuance and settlement framework
- a verification layer
- a product SDK and white-label platform

### What Halcyon Is Not

- a broad consumer wealth app on day one
- a generic derivatives exchange
- a single-product protocol whose entire fate depends on one flagship note

## 3. Problem

There are four linked problems in today's on-chain structured-products market.

### 3.1 Opaque pricing destroys trust

Most crypto structured products historically relied on off-chain quote engines, market makers, or opaque volatility surfaces. Buyers could not independently verify the price they were being offered. That makes the product feel extractive and fragile even when the frontend looks polished.

### 3.2 Building one product requires a full stack

To launch even a single note product, a team needs:

- a pricing engine
- state management for live policies
- settlement logic
- vault accounting and fee routing
- keeper infrastructure
- frontend quoting and issuance flows
- auditability and replay

That is too much for most wallets, yield apps, tokenized-asset issuers, and treasury managers to build in-house.

### 3.3 Existing on-chain yield products are not truly defined-outcome

Most crypto yield products are some mix of:

- passive lending
- staking
- covered-call or options vaults
- strategy wrappers with hidden or changing risk

These products do not give the user a clean answer to the question: "What happens to my money under a few specific scenarios?"

### 3.4 Distribution and compliance are mismatched

A direct-to-retail structured-note business faces immediate friction:

- user education
- wallet trust
- legal and compliance gating
- capital formation
- thin secondary liquidity

The result is that a technically sophisticated product can exist without a commercially viable go-to-market path.

## 4. Solution

Halcyon provides a shared on-chain kernel for policy lifecycle and capital accounting, with product-specific pricing programs layered on top.

The current architecture already reflects this split:

- the kernel owns money, reservations, policy headers, fee routing, and settlement
- product programs own pricing and product-specific rules
- the frontend uses a wallet-free preview flow via `simulateTransaction`

This matters commercially because it means Halcyon can support a family of adjacent products without rebuilding the entire stack each time.

### 4.1 Core Product Offer

Commercially, Halcyon should be packaged as four modules:

1. Pricing Engine
   Deterministic on-chain quote calculation for each supported product family.
2. Issuance Rail
   Policy creation, premium handling, reservation accounting, and settlement.
3. Verification Layer
   Quote replay, open pricing logic, audit trails, and partner-facing transparency.
4. Distribution Toolkit
   SDKs, white-label frontend components, monitoring, and operational runbooks.

### 4.2 Initial Product Stack

Halcyon should be presented as a three-part stack, with each part serving a different commercial role.

#### 1. Hero Demo: IL Protection

Use IL Protection to create the immediate "mic drop" moment:

- connect wallet
- detect the user's Raydium SOL/USDC LP
- quote matching synthetic cover
- issue protection in one flow

This is the most legible end-user experience in the current product set. It proves Halcyon can protect a real on-chain position a user already owns.

#### 2. Strategic Flagship: Tokenized-Equity Worst-of-3 Autocall

Use the SPY/QQQ/IWM flagship as the company-defining product:

- it ties Halcyon to a real TradFi category
- it connects Halcyon to tokenized equities and RWAs rather than a purely crypto-native niche
- it creates a stronger partner and investor story
- it demonstrates the ceiling of the pricing engine

This should be the primary narrative in partner, investor, and long-term platform conversations, even if distribution initially happens only through licensed or restricted channels.

#### 3. Bridge Product: SOL Income Notes

Use SOL note products where Halcyon needs a lower-friction crypto-native deployment path:

- easier partner pilots
- shorter feedback loops
- recurring issuance cadence
- useful fallback if RWA distribution is delayed

SOL is therefore a bridge product, not the center of the company story.

### 4.3 Why Customers Buy

Partners do not buy Halcyon because they love NIG distributions or Bessel functions.

They buy because Halcyon gives them:

- a differentiated earn product they can launch faster
- transparent quotes they can show users
- settlement rails they do not need to build themselves
- auditability they can use in risk, ops, and compliance discussions
- a path to launch new note products from the same base stack

## 5. Product Strategy

### 5.1 Product Roadmap

Halcyon should commercialize in three layers.

### Layer 1: Hero Demo

Use IL Protection as the 60-second proof:

- connect wallet
- detect LP position
- quote matching synthetic cover for that position
- issue or simulate issuance immediately

Goal: prove Halcyon can price and protect a real on-chain position a user already owns.

### Layer 2: Strategic Commercial Narrative

Use the SPY/QQQ/IWM worst-of-3 autocall as the flagship partner story.

Goal: win tokenized-asset platforms, licensed distributors, and sophisticated partners by showing that Halcyon can power real tokenized-equity structured products.

### Layer 3: Bridge and Expansion Products

Use crypto-native note products to bridge into broader distribution where needed:

- SOL income notes
- BTC or ETH note families
- principal-backed crypto notes
- buffered crypto structures

Goal: create lower-friction deployment paths and a broader menu of products built on the same rail.

### 5.2 Product Prioritization

### Strategic Priority 1: Tokenized-Equity Structured Notes

Why:

- stronger market narrative
- larger long-term revenue pool
- direct connection to tokenized-equity and RWA growth
- better investor and enterprise story
- more defensible than another crypto-native yield product

Why not unrestricted retail first:

- harder legal path
- longer tenor
- greater partner and compliance complexity
- likely needs licensed or restricted distribution channels first

### Demo Priority: IL Protection

Why:

- strongest live demo moment
- deeply legible to Solana-native users
- tied to a real wallet state and a real position
- recurring 30-day cadence creates a clearer repeat-use story than an 18-month note

What it is not:

- not the entire company
- not the only long-term product family
- not necessarily the highest-value enterprise narrative

### Commercial Bridge: Crypto-Native Income Notes

Examples:

- SOL income notes
- BTC or ETH income notes
- principal-backed variants
- buffered upside or downside-protected structures

Why:

- lower-friction pilots
- same lifecycle surface
- same infrastructure base
- useful if RWA partner distribution takes longer than expected

### Deprioritized: SOL as the Company-Defining Narrative

SOL products can be useful commercially, but they should not be the lead story:

- they feel smaller than the RWA/tokenized-equity opportunity
- they are easier to dismiss as another crypto yield wrapper
- they do not capture the same TradFi category pull as the flagship

## 6. Market Opportunity

The useful market is not "all structured products" and not "all of DeFi."

The realistic serviceable market is the set of Solana-native distribution partners that want to add defined-outcome products without building the infrastructure from scratch.

The strategic point is that Halcyon is not creating demand for tokenized-equity products from zero. It is trying to become the infrastructure layer that sits on top of a live RWA and tokenized-equity market and makes those assets programmable into note products.

### 6.1 Target Market Segments

#### Segment A: Tokenized-Asset Platforms and RWA Apps

Examples:

- stock and ETF token platforms
- RWA apps
- on-chain brokerage-like experiences

Needs:

- note wrappers around existing assets
- programmable payoff structures
- auditability and replay
- distribution-ready structured products

#### Segment B: Structured-Product Distributors and Licensed Issuers

Examples:

- licensed wrappers
- broker-dealer or authorized financial counterparties
- regional distributors

Needs:

- issuance software
- auditable pricing
- lower engineering cost
- programmable settlement and reporting

#### Segment C: Wallets and Yield Apps

Examples:

- wallets with an `Earn` tab
- treasury dashboards
- yield aggregators
- consumer-facing crypto savings products

Needs:

- a differentiated yield product
- clean frontend integration
- low operational overhead
- transparent risk explanation

#### Segment D: DAOs and Treasuries

Examples:

- protocol treasuries
- foundations
- sophisticated crypto capital pools

Needs:

- defined-outcome deployment of idle stablecoins
- ring-fenced risk products
- transparent monitoring and reporting

### 6.2 Bottom-Up Market View

The first viable business does not require billions in TVL.

A realistic 24-month target is:

- 8 to 15 live partners
- 1 to 3 active products per partner
- $10 million to $50 million annual notional per partner

That implies:

- low case: $80 million annual platform notional
- base case: $150 million annual platform notional
- high case: $500 million annual platform notional

At a blended platform take of 50 to 150 basis points, that is enough to support a real software business before Halcyon takes significant underwriting risk.

## 7. Customer and Buyer Personas

### 7.1 Primary Economic Buyer

Head of product, GM, or founder at a tokenized-asset platform, licensed issuer, or wallet partner.

What they care about:

- shipping a new product quickly
- increasing balances and retention
- reducing engineering lift
- avoiding reputational blowups from opaque pricing

### 7.2 Primary Technical Buyer

Engineering lead or protocol lead.

What they care about:

- integration complexity
- security posture
- deterministic behavior
- auditability
- operational runbooks

### 7.3 Risk and Compliance Stakeholder

General counsel, operations lead, external advisor, or compliance partner.

What they care about:

- how the quote is generated
- what is on-chain versus off-chain
- how pause controls and monitoring work
- what jurisdictions and user segments are allowed

### 7.4 End User

The partner's end user, not Halcyon's direct day-one customer.

What they care about:

- what do I put in
- what can I get out
- when can I redeem
- when can I lose money
- why should I trust the quote

## 8. Go-To-Market Strategy

### 8.1 Positioning

The correct positioning is:

"Halcyon is the auditable issuance rail for tokenized-equity and other defined-outcome products."

Not:

- "the first on-chain autocallable"
- "a structured products app for everyone"
- "quant math that fits on-chain"

The technical line belongs in proof and credibility, not in the headline.

### 8.2 Sales Motion

### Phase 1: Design Partners

Target 20 high-fit conversations with:

- tokenized-asset teams
- licensed issuers or distribution partners
- Solana wallets
- treasury operators

Success criteria:

- 5 active design partners
- 2 signed pilot agreements or equivalent commitments
- 1 credible partner path for tokenized-equity distribution
- 1 live pilot on devnet or limited mainnet distribution

### Phase 2: Pilot Distribution

Offer a white-label or embedded product launch. The initial pilot can take one of two forms:

- one tokenized-equity flagship through a licensed or restricted channel, or
- one IL-protection or crypto-native bridge product through a lower-friction wallet partner
- capped issuance
- close monitoring
- partner-branded frontend

Success criteria:

- repeat issuance over multiple cycles
- user engagement beyond first note purchase
- partner willingness to expand product scope

### Phase 3: Platform Expansion

Once pilots prove traction:

- add second product family
- add partner self-serve tooling
- standardize servicing and reporting

Success criteria:

- multiple partners live simultaneously
- repeat issuance without founder-led setup each time
- revenue concentration reduced across customers

### 8.3 Distribution Channels

- founder-led outbound to ecosystem teams
- demo-driven inbound from hackathon and research visibility
- strategic integrations with wallets and tokenized-asset apps
- content showing quote replay, settlement, and verification

### 8.4 What Must Be Proven Before Full Launch

- at least 3 to 5 partners express willingness to pilot
- at least 1 partner commits engineering time
- the IL wallet-detect-to-cover demo is understandable without explanation
- there is at least one credible licensed or partner-distributed path for the tokenized-equity flagship
- the first product is simple enough for a non-quant PM to describe

## 9. Business Model

Halcyon should not rely on underwriting spread alone as the first business.

The direct vault economics in the current product reports are useful proof that the products can be economically coherent, but they are too thin and capital-intensive to be the only revenue model at this stage.

For example, even a conservative direct vault edge of roughly $4.50 to $6.50 per $1,000 note translates to 45 to 65 basis points of gross economics per note cycle before overhead. That is useful, but it does not by itself justify a venture-scale company unless distribution volume and capital base become very large.

The business should therefore have four revenue layers.

### 9.1 Integration Fees

Charge one-time fees for:

- partner onboarding
- product configuration
- white-label setup
- custom reporting and controls

Target range:

- $25,000 to $75,000 per partner

### 9.2 Platform Fees on Notional Issued

Charge a fee based on annual notional issued through Halcyon.

Target range:

- 50 to 150 basis points of notional

Notes:

- lower end for standard products
- higher end for complex or fully managed deployments

### 9.3 Servicing and Monitoring Fees

Charge ongoing fees for:

- keeper operations
- monitoring
- operational reporting
- incident response and pause management

Target range:

- $2,000 to $10,000 per month per partner

### 9.4 Optional Underwriting and Revenue Share

Where Halcyon or a partner-sponsored vault provides capital:

- share issuance economics
- share retained coupon haircut and margins
- potentially earn performance-based upside

This should be optional and not required for the first commercial wins.

## 10. Financial Model

The financial model below is an operating model, not an audited forecast.

It assumes Halcyon is primarily an infrastructure and servicing company in the first 24 months, with underwriting as optional upside.

### 10.1 Base Case Pricing Assumptions

- average partner setup fee: $50,000
- average servicing fee: $5,000 per month
- blended platform fee: 1.00% of notional issued
- gross margin on software and servicing revenue: high
- gross margin on underwriting revenue: lower and more variable

### 10.2 Three-Year Base Case

#### Year 1

- 3 live partners
- $15 million annual notional
- $150,000 setup revenue
- $150,000 platform-fee revenue
- $180,000 servicing revenue
- total revenue: $480,000

Primary goal:

- prove partner demand, not maximize profit

#### Year 2

- 8 live partners
- $80 million annual notional
- $250,000 setup revenue
- $800,000 platform-fee revenue
- $480,000 servicing revenue
- total revenue: $1,530,000

Primary goal:

- establish repeatable sales motion and operational leverage

#### Year 3

- 15 live partners
- $250 million annual notional
- $250,000 setup revenue
- $2,500,000 platform-fee revenue
- $900,000 servicing revenue
- total revenue: $3,650,000

Primary goal:

- become the default issuance rail in the category

### 10.3 Upside Case

If Halcyon also captures underwriting revenue on selected products, revenue can expand materially, but that requires:

- more capital
- stronger legal structure
- better risk systems
- deeper partner trust

Underwriting should be treated as margin expansion, not the base case.

## 11. Competitive Positioning

Halcyon's main competition is not another perfect on-chain copy. It is the combination of in-house builds, simpler yield alternatives, and partner inertia.

### 11.1 Alternatives Customers Use Today

- do nothing
- launch another lending or staking wrapper
- rely on an opaque off-chain quote flow
- build a one-off internal product
- use tokenized assets without structured wrappers

### 11.2 Competitive Advantages

#### Verifiable Pricing

Halcyon's strongest asset is that quote generation, replay, and settlement logic can be tied to the same deterministic code path.

#### Reusable Kernel

A partner does not need to rebuild vault accounting, settlement, fee routing, and policy lifecycle for each product.

#### Product Breadth Within a Narrow Family

Halcyon is expandable across adjacent defined-outcome products without becoming an everything protocol.

#### Technical Credibility

The current math and testing work provide a strong proof moat, especially for sophisticated partners and judges.

### 11.3 What Is Not Yet a Moat

- distribution
- liquidity network effects
- licensed compliance wrapper
- brand with end users

The business plan must assume these still need to be built.

## 12. Regulatory and Compliance Strategy

This section is strategic, not legal advice.

The practical conclusion is simple: Halcyon should not assume a frictionless global retail launch.

Recent official guidance reinforces that:

- the SEC stated on January 28, 2026 that tokenized securities remain subject to the federal securities laws
- the FCA continues to actively enforce the UK's crypto financial promotions regime, including action announced on February 10, 2026
- in the EU, MiCA has been in force for certain crypto-asset activity since December 2024, but tokenized securities and related products can still trigger separate regulatory treatment depending on structure

### 12.1 Commercial Implication

Halcyon's launch posture should be:

- infrastructure first
- partner-distributed first
- limited-jurisdiction and limited-segment first
- retail-later, if at all

### 12.2 Initial Compliance Operating Model

- obtain specialist legal advice before public mainnet issuance
- prioritize pilots with sophisticated or restricted user segments
- work through licensed or regulated counterparties where needed
- treat unrestricted retail tokenized-equity distribution as later-stage, but pursue tokenized-equity partner distribution from the start
- keep strong pause, monitoring, and audit controls visible in the product

### 12.3 Why This Still Works as a Business

The business does not need immediate direct consumer distribution to work.

If Halcyon becomes the backend for partners who already have users, it can monetize before it solves every retail compliance question itself.

## 13. Operating Plan

### 13.1 Team

Initial core team requirements:

- founder/CEO-product lead
- protocol engineer
- frontend/integration engineer
- ops/devops and keeper reliability support
- outside legal and compliance advisors

Short term, some of these can be fractional. Over time, partner integrations and operational uptime will matter as much as pricing research.

### 13.2 Product and Engineering Priorities

1. Judge-ready and partner-ready wallet-free demo
2. LP-detection IL demo that feels instantaneous and personal
3. Flagship tokenized-equity partner package
4. Partner SDK and integration docs
5. Monitoring, reporting, and admin tooling
6. Security review and audit posture

### 13.3 Success Metrics

Track:

- number of design partner conversations
- number of pilot agreements
- notional issued
- repeat issuance rate
- quote-to-issue conversion rate
- number of end users completing multiple note cycles
- partner expansion from one product to two

## 14. Risks and Mitigations

### 14.1 Distribution Risk

Risk:

Partners may find the product intellectually interesting but not commercially urgent.

Mitigation:

- sell a simple yield product first
- quantify partner value in balances, retention, and differentiation
- run pilots with tight success criteria

### 14.2 Regulatory Risk

Risk:

Public distribution of note-like products may be restricted or slow.

Mitigation:

- start with partner-distributed and restricted rollouts
- use counsel early
- separate the technology business from direct retail ambitions

### 14.3 Product Complexity Risk

Risk:

The market may not care about sophisticated structures if the frontend value is unclear.

Mitigation:

- lead with outcome language
- keep advanced math below the fold
- use one simple commercial wedge

### 14.4 Model and Hedging Risk

Risk:

The products can be economically coherent in backtest and still break operationally in live trading.

Mitigation:

- conservative caps
- staged rollout
- visible circuit breakers
- kill-switches and per-product pause controls
- third-party audit and replay tooling

### 14.5 Partner Concentration Risk

Risk:

Too much revenue could sit with one or two early partners.

Mitigation:

- diversify integrations
- standardize deployment
- grow the platform layer faster than bespoke services

## 15. Milestones

### 15.1 Next 90 Days

- package the hackathon demo around the LP-insurance hero flow
- rewrite positioning around flagship RWA narrative plus IL demo
- run 20 partner and user discovery conversations
- secure 3 to 5 design partners
- deliver one live or simulated pilot environment

### 15.2 Next 6 Months

- complete security review for the pilot surface
- launch the first partner-facing pilot
- ship partner integration docs and SDK
- prove repeat issuance on one product family
- validate one tokenized-equity partner path

### 15.3 Next 12 Months

- convert pilots into paid deployments
- add a second product family
- standardize servicing and reporting
- expand licensed-partner distribution for tokenized-equity structures

## 16. Financing Plan

Halcyon should treat the next financing round as an execution round, not a research round.

Suggested target:

- $750,000 to $1,250,000 pre-seed or equivalent grants plus angel capital

Use of funds:

- security and audits
- legal and structuring work
- partner integrations
- reliability tooling
- 12 to 18 months of runway

The message to investors or grant reviewers should be:

"The math is already here. The next stage is distribution, compliance packaging, and operationalization."

## 17. Strategic Conclusion

Halcyon has two different stories available to it.

The weak story is:

"We built a sophisticated structured-note protocol and now need users."

The strong story is:

"We built the missing infrastructure layer that lets partners launch transparent defined-outcome products on Solana."

The second story is the company.

The practical business plan is therefore:

- win attention with the technical breakthrough
- commercialize through a simple, repeatable product wedge
- sell to partners before trying to win retail directly
- expand from one note family into a narrow but defensible category

If Halcyon executes this plan, the existing technical work becomes a business. If it does not, it risks remaining a very impressive pricing engine without durable distribution.

## 18. Reference Notes

External regulatory assumptions in Section 12 were informed by:

- RWA.xyz tokenized stocks market pages reviewed March 2026
- Solana xStocks case study published January 19, 2026
- SEC, "Statement on Tokenized Securities," January 28, 2026
- FCA, action against illegal crypto financial promotions announced February 10, 2026
- ESMA guidance and investor communications on MiCA and crypto-asset regulation in force since late 2024

These references are included to anchor launch posture, not as a substitute for legal advice.
