# Halcyon Memo

Last updated: 2026-04-20

## What Halcyon Is

Halcyon is the issuance and pricing rail for on-chain defined-outcome products.

The core asset is not a single note. It is a reusable stack that lets partners:

- quote products on-chain
- verify those quotes before issuance
- issue and settle policies through a shared kernel
- launch new payoff structures without rebuilding the full stack

In practical terms, Halcyon is trying to do for structured products what Stripe did for payments infrastructure: turn a complex back office into a programmable API and operating layer.

## Why This Matters

On-chain structured products have historically had two problems:

1. pricing was opaque and usually off-chain
2. distribution was too thin to justify the complexity

Halcyon addresses the first problem directly. Quotes, replay, and settlement can be tied to the same deterministic pricing path.

That matters more now because tokenized equities and other RWAs are starting to create a real substrate for structured products on Solana. Halcyon's flagship SPY/QQQ/IWM worst-of-3 autocall matters because it connects the protocol to a real TradFi category, not just another crypto-native yield wrapper.

## The Product Hierarchy

Halcyon has three different product roles. They should not be confused.

### 1. Flagship Narrative: Tokenized-Equity Worst-of-3 Autocall

This is the company-defining product.

Why it matters:

- strongest market story
- real RWA / tokenized-equity angle
- closest bridge to a large TradFi category
- best investor and enterprise narrative

This is the product that says Halcyon is more than a crypto yield experiment.

### 2. Hero Demo: IL Protection

This is the strongest live demo.

The magic moment is simple:

- connect a wallet
- detect a Raydium SOL/USDC LP
- quote matching synthetic IL cover
- issue protection immediately

That is more legible in 60 seconds than an 18-month autocall. It proves Halcyon can protect a real on-chain position a user already owns.

### 3. Bridge Product: SOL Notes

SOL note products are useful, but they are not the story.

They matter because:

- they are easier to pilot
- they have shorter feedback loops
- they offer a lower-friction crypto-native path if RWA distribution is slow

They should be treated as bridge products, not the center of gravity.

## The Business

The business is not "sell one note directly to retail."

The business is:

`Halcyon = infrastructure for partners to launch auditable defined-outcome products.`

Target customers:

- tokenized-asset platforms
- licensed issuers and distributors
- wallets and yield apps
- DAOs and treasuries

What they buy:

- pricing engine
- issuance rail
- verification layer
- SDKs, white-label flows, and operational tooling

How Halcyon makes money:

- integration fees
- platform fees on notional issued
- servicing / monitoring fees
- optional underwriting revenue share

The key point is that underwriting spread alone is not enough to build the company. Halcyon works as a business when the software and partner layer comes first.

## Go-To-Market

Halcyon should go to market in this order:

### 1. Win Attention

Use the IL wallet-detect-to-cover flow as the demo that judges, users, and partners instantly understand.

### 2. Win Credibility

Use the tokenized-equity flagship to show that the infrastructure can support serious structured products, not just simple crypto wrappers.

### 3. Win Distribution

Sell to partners first:

- tokenized-asset teams
- licensed issuers
- wallets with an `Earn` surface

This is a B2B / B2B2C motion before it is a retail motion.

## Regulatory Reality

Halcyon should not assume frictionless global retail distribution for tokenized-equity products.

The implication is straightforward:

- lead the company story with tokenized equities
- pursue partner and licensed distribution paths early
- treat unrestricted retail tokenized-equity distribution as later

That is not a weakness. It is simply the correct packaging for the market as it exists.

## What Must Be True

To become a business, not just a protocol demo, Halcyon needs to prove:

- the IL demo is instantly understandable
- at least one credible partner path exists for the tokenized-equity flagship
- partners want Halcyon as infrastructure, not just as an interesting app
- the protocol can support repeat issuance, not one-off novelty

## Bottom Line

The weak framing is:

"Halcyon is a complex note app looking for users."

The strong framing is:

"Halcyon is the missing infrastructure layer for on-chain defined-outcome products, with a live LP-insurance demo and a flagship tokenized-equity structured product."

That is the memo.
