# Things to do before launch

Running punch list. Covers the items that came up during the L5 audit
remediation / frontend / branding work and are deferred for later.
Audit-specific items that the kernel owns are tracked in
[`docs/audit/OPEN_QUESTIONS.md`](./audit/OPEN_QUESTIONS.md); read that
alongside this list.

Ordered by when to handle them, not by effort.

---

## 1 — Blockers for mainnet deploy

Must be done before anyone types `anchor deploy --provider.cluster mainnet`.

### 1.1 Helius API key hardening

Current state: `frontend/.env.local` has `NEXT_PUBLIC_RPC_URL_MAINNET`
and `NEXT_PUBLIC_LP_DETECTION_RPC` pointing at Helius. Because they use
the `NEXT_PUBLIC_*` prefix, the key is baked into the client bundle.

**Tier 1 (5 min, must-do)**: Helius dashboard → API Keys → set
Allowed Origins to the production domain + localhost + Playwright's
`:3001`. Neutralises the leak risk even though the key is in the
bundle.

**Tier 2 (30–60 min, recommended)**: server-side RPC proxy via a
Next.js API route at `frontend/app/api/rpc/route.ts`. Key moves to
`HELIUS_API_KEY` (no `NEXT_PUBLIC_` prefix). Frontend `rpcUrl` becomes
`/api/rpc`. Client bundle never sees the key. Adds ~30 ms per RPC call.

**Tier 3 (defer)**: edge-worker proxy with rate limiting. Overkill
until traffic justifies it.

### 1.2 F1 flagship hedge keeper — live Jupiter submit path

`keepers/flagship_hedge_keeper/` ships as scaffold-only (dry-run).
Live path not written. Flagship stays paused-public until this
completes a successful devnet rebalance cycle end-to-end (keeper writes
→ Jupiter executes → kernel records → HedgeBookState reflects the
trade). See `OPEN_QUESTIONS.md` §"Audit F1 follow-up" for the unpause
predicate.

### 1.3 Program deployment SOL + upgrade authority

- Deploy cost: ~25–40 SOL reclaimable program rent + buffer accounts.
  Back up deploy keypair (and all four program keypairs) to password
  manager + encrypted offline copy before touching mainnet.
- Upgrade authority on a hardware wallet (Ledger) for mainnet. Not a
  hot file on the VPS. Never pass `--final` (permanently locks the
  SOL).
- See `docs/runbooks/keeper_hosting.md` §8 for the deploy sequence.

### 1.4 Paid RPC + Pinata tier

- Helius Developer plan ($49/mo) for keepers. Public RPCs will drop
  mainnet transactions silently.
- Pinata paid tier ($20/mo) — delta keeper pins at 30 s cadence, free
  tier's 100 pins/month blows through in under an hour.

### 1.5 Keeper keypairs generated + funded + registered

- Generate five mainnet keeper keypairs on an air-gapped machine:
  observation, regression, delta, hedge, regime.
- Register each in `KeeperRegistry` via the admin multisig.
- Fund each with ~1 SOL mainnet.
- Transfer keypair files to VPS via a confidential channel.
- See `docs/runbooks/keeper_hosting.md` §3 for the full procedure.

### 1.6 Admin multisig

Current: `ProtocolConfig.admin` is a single keypair. For mainnet this
has to be a multisig (Squads, Realms, or equivalent). Rehearse one
harmless admin instruction through the multisig UX before launch.

### 1.7 Alerting wired

- Prometheus/Grafana Cloud free tier for keeper heartbeats.
- Pages configured per `docs/runbooks/keeper_hosting.md` §5:
  - Any keeper systemd unit in `failed` state
  - Heartbeat > 5× `scan_interval_secs`
  - Keeper keypair SOL balance < 0.1 SOL
  - RPC p95 latency > 2 s sustained 5 min
  - Pinata pin failure, 3 consecutive

### 1.8 Five pre-existing failing tests in `halcyon_flagship_quote`

Tests that fail on both `main` and the post-refactor branch:

- `obs1_table_vs_exact`
- `frozen_predict_vs_live_quote_accuracy`
- `tapered_quote_sanity`
- `projected_filter_k15_hits_relaxed_anchor_band`
- `public_lookup_quote_matches_exact_leg_tables`

Pattern: tables vs. exact-leg comparisons blowing the tolerance
threshold by bounded amounts (e.g. `max_w_err=52146ppm` vs. 10000ppm).
Smells like calibration drift or a threshold that needs tightening —
not a correctness regression. **Math decision, Dom-only.** Needs
review before mainnet flagship issuance.

---

## 2 — Pre-launch frontend polish

Noticeable issues that hurt first impressions. Do before opening
public access.

### 2.1 Target user line on landing page

Currently the landing page doesn't name who the product is for. Add
one sentence above the CTA: *"For: crypto-native holders of $10k–500k
USDC who want TradFi-style equity yield without opening a brokerage
account."* or whatever target user resonates. Moves four dimension
scores in the product roast.

### 2.2 Monetization line on landing page

Currently silent on how Halcyon makes money. Add: *"Halcyon retains
0.3% of every premium to fund underwriting capital and protocol
development."* Fees already exist in `ProtocolConfig.senior_share_bps /
junior_share_bps / treasury_share_bps`; just needs to be surfaced.

### 2.3 Landing-page live quote via WASM

`app/halcyon_wasm.wasm` can price flagship notes in the browser with
zero wallet and zero network config. Embed it on `/` as a
`<QuoteSimulator />` component — notional slider + live coupon. Closes
the "we talk about on-chain pricing, we don't show it" gap.

Scope: ~4–6 hours. Webpack config for wasm-pack output, client-side
lazy-load, a minimal UI. See `app/pricing.js` + `app/wasm_loader.js`
for the existing JS-side glue to port.

### 2.4 IL Protection: WASM-powered live premium

Same as 2.3 but for the IL Protection detected flow. Currently the
detected flow calls `simulatePreview` against the on-chain program.
Adding a WASM-powered live premium that updates as the user types
would match `app/page_il.jsx:166` behaviour and let a judge see the
premium change without any wallet. Bundles with 2.3.

### 2.5 Deep links / shareable quote URLs

Currently `/flagship`, `/il-protection`, `/sol-autocall` are stateless
forms. A buyer can't share "my $100k 18-month SPY/QQQ/IWM quote" with
a friend. Push the notional + slippage into URL search params so the
state is shareable. Growth lever.

### 2.6 Token amounts with USD equivalents

"Annualised coupon 15%" is abstract. Next to it show "≈ $15,000/yr on
a $100k notional" once the user has entered one. Tiny change, real
comprehension upgrade.

### 2.7 Mobile real-device test

Playwright covers tablet + mobile viewport widths but no actual-phone
testing. Crypto traffic is ~50% mobile. Test the wallet flow on
Phantom Mobile + Solflare Mobile + Backpack Mobile on a real iPhone
and Android device before launch.

### 2.8 Coverage gaps in Playwright / anchor tests

- TS anchor tests for audit F5 paused-product rejection + F7
  invalid-ALT-owner rejection. On-chain constraints exist; test cases
  don't.
- End-to-end anchor test for F4b Ed25519 signature + F4a Pinata
  round-trip + F2 Pyth publish_time monotonicity.

---

## 3 — Devnet demo items (before Colosseum judging)

Strictly for the submission, not for mainnet launch.

### 3.0 Devnet bring-up (2026-04-21) — COMPLETE

Full kernel + 3-product bring-up landed on devnet. The judge-facing path now
uses a Halcyon mock-USDC mint so wallets can self-fund through `/faucet`
instead of relying on Circle's rate-limited devnet faucet.

**Programs (identical IDs on Anchor.toml's `[programs.devnet]` /
`[programs.localnet]`):**

- `halcyon_kernel` — `H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF`
- `halcyon_il_protection` — `HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g`
- `halcyon_sol_autocall` — `6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8`
- `halcyon_flagship_autocall` — `E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc`

The old SOL autocall `.bss` section-strip workaround is obsolete after the
POD-DEIM table precompute refactor. A clean `anchor build` should now be the
deploy path; do not reintroduce manual ELF section stripping.

**Admin state:**

- `ProtocolConfig` — `4kvGdC4UE3SeNWEDEEcvTUHet9uwVUQTAFMN3mWrt9Fr`
- `init-payment-mint` initializes the vault and treasury token accounts for the
  current mock-USDC mint, then rotates treasury/hedge-defund destinations to
  the admin's mock-USDC ATA.

**Product registry entries:**

- SOL Autocall — `E5sWhQTx1vcRthQzejKhD9FpjQA1wR945vKyswNxBUb8`
- IL Protection — `6Amqc2idd8dZAAwV14qknMZZ3Hjvw5RyK6JHRZpx5hSW`
- Flagship Autocall — `G5egRETFHGosHu6k2ptpHrxe4Y2DrSj4XnhKN6tVPeLt`

**Keepers registered in `KeeperRegistry`** (role code → pubkey →
keypair path):

- `0` Observation — `G1ZcTGwr2uoBfLjgRq5Dx23FvNmUDBb33okRDsEHHDQQ` — `ops/devnet_keys/observation.json`
- `1` Regression  — `Cn1hEo1bqmc2uoyGX6fWyGCHizemfG1iwG5yBxu7tpsw` — `ops/devnet_keys/regression.json`
- `2` Delta       — `4NPmg2gLvqsYToK8ff3WEyvyq2PrdL6ocuHAMLDCVxoq` — `ops/devnet_keys/delta.json`
- `3` Hedge       — `2tPQ2zVkEzWxghuZEsassRW7yg5x1DhGjXRyLdLZNnBs` — `ops/devnet_keys/hedge.json`
- `4` Regime      — `FeX66ZSzW67DJL7MQsyXCeTctsXFb1fX4emArQyYi25y` — `ops/devnet_keys/regime.json`

Each keeper keypair is funded with 1 SOL. `ops/devnet_keys/` is
gitignored.

**Frontend `.env.local`** has all 4 devnet program IDs and the 5
Pyth push-oracle `PriceUpdateV2` addresses (shard 7).

**CLI gap closed during bring-up:** added `register-flagship-autocall`
subcommand to `tools/halcyon_cli` (SDK had
`register_flagship_autocall_ix` but the CLI didn't expose it).

**What's still needed for a live demo (Dom-only):**

- Create the mock-USDC mint with `tools/mock_usdc_faucet`, run
  `halcyon init-payment-mint`, set `USDC_MINT` / `NEXT_PUBLIC_USDC_MINT_DEVNET`
  to that mint, and expose the faucet URL through
  `NEXT_PUBLIC_MOCK_USDC_FAUCET_URL`.
- Start the 5 keepers. `keepers/price_relay/` is already running
  (shard 7, all 5 feeds posting). The 4 quant keepers
  (`observation_keeper`, `regression_keeper`, `delta_keeper`,
  `regime_keeper`, `hedge_keeper`) need to be invoked with each
  role's keypair pointing at the devnet Helius RPC.
- Hit issuance → settlement once per product to record a
  `PolicyHeader` for the demo.

### 3.1 Pyth devnet equity feed verification

Confirm SPY/QQQ/IWM Pyth feeds actually exist and publish on devnet.
If they don't, the flagship quote path can't resolve on devnet — need
a mock-Pyth fallback or switch the flagship demo to mainnet.

### 3.2 Cross-chain / xStocks caveat for flagship demo

Backed Finance bSPY/bQQQ/bIWM only exist on mainnet. Flagship hedge
keeper's Jupiter route won't resolve on devnet. For the Colosseum
demo, either:

- Mainnet deploy with the flagship hedge keeper in dry-run mode
  (shows the quote + issuance works, skips showing the hedge trade), OR
- Devnet deploy of everything except the hedge execution; demo the
  other three products (IL, SOL Autocall) end-to-end and show flagship
  pricing + issuance but not live hedge.

Document which path before recording the demo.

### 3.3 Runtime guard + localnet default

Dev build defaults to `cluster = "localnet"` which has no pinned
genesis hash. Runtime guard already passes through this via
`status: "skipped"`, but if you change this for production, make sure
the default never lands a user on a cluster without a matching RPC.

### 3.4 Ed25519 verify script in front of judges

`research/tools/verify_aggregate_delta.py` was written for auditors
but it's also a great prop for the pitch: live, show the script
running against a real on-chain AggregateDelta, verify the signature,
fetch the IPFS artifact, round-trip the Merkle root. 30-second sizzle
reel that proves "on-chain pricing you can re-run from scratch."

---

## 4 — Post-launch cleanup

Fine to defer until Halcyon has survived its first 30 days.

### 4.1 Narrative doc sweep

`integration_architecture.md`, `ARCHITECTURE.md`, `THREAT_MODEL.md`
were written while the code was catching up. Now the code backs every
claim (audit F2/F3/F4a/F4b/F5 all landed). Walk every page, remove
hedging language ("mostly deterministic", "partial auditability",
"largely fixed-point"). The claims are now unqualified.

### 4.2 Quasar migration (CU optimization)

Covered in `docs/audit/OPEN_QUESTIONS.md` §"Post-submission
optimization". ~3–5 focused days to port kernel + 4 products from
Anchor 0.32.1 to Quasar. Expected ~30% CU reduction on the dispatch
layer, ~30% program-rent reduction. **Only do this if CU becomes a
binding constraint.**

### 4.3 Brand — data-viz + chart treatments

The Halcyon palette covers solid colour usage but no charts are wired
yet. When the portfolio view starts showing coupon/observation history
or the vault view shows TVL curves, pull from the data-viz tokens in
`app/tokens.css` (`--d-c-1..6` categorical, `--d-s-1..6` sequential
blue ramp).

### 4.4 Flagship hedge keeper monitoring

Once F1 live-submit lands, add:
- Prometheus counters for hedge trade fills
- Jupiter price-sanity-check reject rate
- AggregateDelta → hedge-execution lag percentile
- `last_rebalance_ts` watchdog (alert if no rebalance for > cadence × 2)

### 4.5 Audit remediation documentation reconciliation

`docs/audit/L5_REMEDIATION_SUMMARY.md` tracks what landed this cycle.
After the F1 live-submit pass and TS test backfill, update it to
show all 7 findings fully closed rather than 5 closed + 2 scaffolded.

### 4.6 xStocks route-depth monitoring

Before unpausing flagship public issuance, verify Jupiter has enough
xStocks (bSPY / bQQQ / bIWM) liquidity for the sizes Halcyon will
quote. Route the hedge keeper through a dry-run simulation at the
largest notional the protocol allows per-position; if the price
impact exceeds `JUPITER_PRICE_SANITY_BPS`, the hedge will refuse to
execute and flagship becomes unhedged.

---

## 5 — Nice to have

### 5.1 Pinocchio migration (alternative to Quasar)

Covered in `docs/audit/OPEN_QUESTIONS.md`. Bigger CU savings than
Quasar but requires hand-writing account parsers. Only consider after
Quasar if still constrained.

### 5.2 `/brand-design` refinement pass

Halcyon palette is shipped via `app/tokens.css` + `globals.css`. Logo
(kingfisher) is ported. Typography (Instrument Serif + Sans +
JetBrains Mono) is wired. Brand book (`brand.md`) still says
"deferred" — worth a 1-hour pass to mark it done and document the
Halcyon Blue / Rust / Paper decisions.

### 5.3 Nav tickers from Pyth Hermes

`app/app_shell.jsx` had a top-bar SPY/QQQ/IWM/SOL price ticker pulling
from Pyth Hermes. Not ported into the Next.js frontend. Would fit
nicely in the header; client-side subscription to Hermes is cheap.
Visual parity with the WASM app.

### 5.4 WasmStatus footer pill

`app/app_shell.jsx` has a "solmath-core 0.1.2 ● wasm" pill in the
sidebar footer that lights up green when the WASM module loads.
Port alongside 2.3 / 2.4.

---

## Cross-references

- [`docs/audit/OPEN_QUESTIONS.md`](./audit/OPEN_QUESTIONS.md) — audit + operations punch list
- [`docs/audit/L5_REMEDIATION_SUMMARY.md`](./audit/L5_REMEDIATION_SUMMARY.md) — what landed in the audit remediation cycle
- [`docs/runbooks/keeper_hosting.md`](./runbooks/keeper_hosting.md) — keeper deploy + ops runbook
- [`docs/runbooks/mainnet_runbook.md`](./runbooks/mainnet_runbook.md) — mainnet launch sequence
- [`docs/audit/how_to_verify_aggregate_delta.md`](./audit/how_to_verify_aggregate_delta.md) — auditor verification procedure
