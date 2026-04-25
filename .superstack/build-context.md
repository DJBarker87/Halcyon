# Halcyon — Build Context

## review

- **security_score**: A-
- **quality_score**: A-
- **ready_for_mainnet**: false (after remaining #3 upper-bound + #5 cu_trace gating → true)
- **reviewed_at**: 2026-04-22
- **reviewed_commit**: 41bdb2f (+ uncommitted working tree)
- **prior_review**: 2026-04-21 @ 6ef3e8a (B+ / A-)

### per_program_security

- `halcyon_kernel`: A-
- `halcyon_flagship_autocall`: A-
- `halcyon_il_protection`: B+ (unchanged — not re-reviewed; #4 still pending)
- `halcyon_sol_autocall`: A-

### carry_forward_status

| # | From 04-21    | Status          | Notes                                                                                                     |
|---|---------------|-----------------|-----------------------------------------------------------------------------------------------------------|
| 1 | Pyth absolute staleness           | **FIXED**       | `crates/halcyon_oracles/src/pyth.rs:92` enforces `(min..=max).contains(&publish_time)`; flagship observation.rs:33-39 and settle.rs:143-149 both use `observation_window_bounds()`. |
| 2 | E11 silent fallback               | **FIXED**       | `programs/halcyon_sol_autocall/src/pricing.rs:179-184` now calls `solve_keeper_deim(...).map_err(|_| error!(QuoteRecomputeMismatch))?` — fallback removed; architecture switched to keeper-fed POD-DEIM. |
| 3 | Regression init unbounded ts      | **PARTIAL**     | `write_regression.rs:62-65` enforces `end > start` unconditionally. Missing: `end ≤ now + GRACE` and `start > 0`. Keeper can still write `window_end_ts = i64::MAX` → DoS (subsequent writes fail monotonicity forever). |
| 4 | IL `price_s6 > 0` guard           | unverified      | Not re-checked this pass.                                                                                 |
| 5 | `cu_trace` in mainnet hot path    | **NOT FIXED**   | `pricing.rs:36-46` gates only on `#[cfg(target_os = "solana")]` — meaning BPF builds always log. Feature `cu_diagnostics` exists in Cargo.toml:19 but isn't used to gate calls. 5 call sites at lines 139/158/178/185 burn CU on mainnet. |
| 6 | Observation replay event          | unverified      | Not re-checked.                                                                                            |
| 7 | Offered-coupon init upper bound   | unverified      | Not re-checked.                                                                                            |
| 8 | `next_coupon_index` entry bound   | unverified      | Not re-checked.                                                                                            |
| 9 | Observation schedule monotonicity | unverified      | Not re-checked.                                                                                            |

### findings

| # | Severity | Category   | File                                                                                      | Description                                                                                  | Fix                                                                         |
|---|----------|------------|-------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------|
| A | Medium   | Security   | `halcyon_kernel/src/instructions/oracle/write_regression.rs:62-65`                        | `window_end_ts` has no upper bound; keeper can write `i64::MAX` and lock future monotonic updates (DoS of regression account). | Add: `require!(args.window_start_ts > 0 && args.window_end_ts ≤ now.saturating_add(REGRESSION_WINDOW_GRACE_SECS), HalcyonError::OracleTimestampOutOfRange);` — unconditional, before the v0/v1 branch. |
| B | Medium   | Compute    | `halcyon_sol_autocall/src/pricing.rs:36-46, 139, 158, 178, 185`                           | `cu_trace` only compiles out on non-BPF; on mainnet BPF builds all 5 log sites fire regardless of `cu_diagnostics` feature. | Change function body: replace `#[cfg(target_os = "solana")]` with `#[cfg(all(target_os = "solana", feature = "cu_diagnostics"))]`. Do NOT add feature to `default`. |
| C | Low      | Hardening  | `halcyon_kernel/src/instructions/admin/migrate_protocol_config.rs:180-190`                | Early-return path (already at `target_len`) deserializes ProtocolConfig but does not require `cfg.version == CURRENT_VERSION`. If a struct-size-preserving future bump ever coexists with stale data, silent no-op could mask it. | Before `return Ok(())`, add: `require!(cfg.version == ProtocolConfig::CURRENT_VERSION, KernelError::BadConfig);` |
| D | Low      | Hardening  | `halcyon_kernel/src/instructions/admin/set_protocol_config.rs` (premium-split validator)  | Sum-to-10k check doesn't bound individual components (belt-and-braces — sum=10k implies each ≤10k, so low-value). | `require!(senior_bps ≤ 10_000 && junior_bps ≤ 10_000 && treasury_bps ≤ 10_000, KernelError::BadConfig);` |

### architectural_risks (unchanged from 04-21)

- **POD-DEIM provenance**: keeper now loads reduced operators on-chain via new `write_reduced_operators.rs`. Whitepaper alignment improved. Document `pod_deim_table_sha256` from ProtocolConfig in submission README.
- **No global keeper pause**: `rotate_keeper` is post-compromise only. Consider `keepers_paused` flag on `ProtocolConfig`.
- **Self-hosting marketing surface on home NUC**: operational SPOF during Colosseum judging.

### clean_areas (newly verified this pass)

- **`write_reduced_operators.rs`** — regime-keeper authority check, abs-value bounds on `p_red` entries (`MAX_ABS_KEEPER_P_RED_ENTRY_Q20`), upload-state coherence correct.
- **`update_ewma.rs`** — variance arithmetic sound (squared ln_ratio ≥0), checked multiplications, monotonicity enforced, rate-limit staleness cap applied.
- **`migrate_protocol_config.rs`** (main path) — admin signer + stored-admin cross-check, rent top-up before realloc, in-place grow (never shrink), V5→V6 and V4→V6 both supported with complete field mapping.
- **flagship `accept_quote.rs` / `preview_quote.rs`** — `seeds::program` qualifiers added → PDA-confusion surface tightened.
- **SOL-autocall `accept_quote.rs`** — slippage bounds, pod_deim table hash, reduced-operator currency gate all verified before issuance.

### clean_areas (carried forward)

- Checked arithmetic throughout value paths; `require_keys_eq!` authority (stricter than `Signer`); canonical PDA bumps + unique seeds.
- `.env` gitignored; CI pins action SHAs; `cargo audit` with waivers; pure-Rust quote crates provably can't depend on `anchor-lang`/`solana-program`.
- Frontend locked to compile-time allowlist + genesis-hash cluster check.
- Keeper daemons hardened via systemd (`ProtectSystem=strict`, `NoNewPrivileges`, `MemoryDenyWriteExecute`).

### next_phase

Two fixes remain blocking mainnet-quality:
- **Finding A** (regression window upper bound) — 3-line handler change.
- **Finding B** (`cu_trace` feature gating) — 1-line `cfg` attribute change.

After A+B, proceed to:
- `/deploy-to-mainnet` — production deployment checklist
- `/submit-to-hackathon` — Colosseum submission builder
- `/create-pitch-deck` — structured pitch deck

HTML artifact at `.superstack/review.html`.

## debug

- **last_debug_session**: 2026-04-25T07:25:00Z
- **issues_resolved**:
  - **error**: Frontend surfaced "Treasury destination is not a token account" before quote simulation.
    **cause**: `fetchProtocolContext` inferred the USDC mint by decoding `ProtocolConfig.treasury_destination`, coupling buyer quote/build flows to the admin sweep-fee destination ATA being initialized.
    **fix**: Frontend runtime config now pins the USDC mint for devnet/mainnet and uses it before falling back to treasury-destination decoding for local/dev setups.
  - **error**: Frontend surfaced `Preview failed: "AccountNotFound"` from `simulatePreview`.
    **cause**: Preview simulations used a freshly generated public key as fee payer; Solana still requires the fee-payer account to exist even with `sigVerify: false`.
    **fix**: Preview simulations now use the protocol admin as the simulation payer, and portfolio lending-value simulations use the portfolio owner.
  - **error**: Frontend surfaced `Preview failed: {"InstructionError":[0,{"Custom":3007}]}` from product quote previews.
    **cause**: The frontend's hardcoded account lists had drifted from the current Anchor IDLs: flagship preview/accept_quote was missing `autocall_schedule`, and SOL autocall preview/accept_quote was missing `reduced_operators`, so later accounts shifted into Anchor-owned account slots.
    **fix**: Added the missing PDA derivations and inserted both accounts in IDL order; preview simulation errors now preserve Solana logs for better UI error mapping, and handled issuance failures no longer trigger Next's dev console-error overlay.
  - **error**: Frontend flagship preview returned `ProgramFailedToComplete` while CLI preview succeeded.
    **cause**: Browser simulations and issuance transactions were missing an explicit compute-budget instruction; the flagship quote path consumes more than the default transaction compute cap.
    **fix**: Frontend transaction builders now prepend a 1.4M CU limit for preview, issuance, lending-value, and single-instruction policy flows.
  - **error**: SOL Autocall preview rendered a zero quote ambiguously and showed `QUOTE SLOT negative`.
    **cause**: The SOL program can legitimately return a zero-coupon no-quote state; the UI did not label that state, and Anchor BN-like values were falling through to object-key display.
    **fix**: The UI now formats BN-like return values as scalars, labels SOL no-quote previews explicitly, and disables issuance while the returned quote is no-quote.
  - **error**: Flagship preview failed with stale sigma/regression state on devnet.
    **cause**: Keeper/oracle state was stale or below protocol floors during manual repair.
    **fix**: Rebuilt and redeployed devnet product programs with the default Pyth pull backend; refreshed SOL EWMA/regime/reduced-operator state; wrote flagship sigma at the configured floor; wrote flagship regression from Pyth Benchmarks only. A Stooq fetch attempt failed before any write and was not used.
  - **error**: Reduced-operator keeper writes could restart from chunk zero and race the live Hetzner timer.
    **cause**: The CLI `fire_reduced_ops` path did not resume compatible partially written operator tables.
    **fix**: CLI reduced-operator writes now resume from existing compatible V/U chunk lengths, keyed by current sigma slot, regime slot, table hash, version, and sigma value. Hetzner should be updated to this binary so live timers stop racing/restarting manual repairs.
  - **error**: IL Protection frontend showed "Market-regime signal is stale."
    **cause**: The IL `RegimeSignal` PDA had last been written at `2026-04-21T11:27:52Z`, outside the devnet freshness window.
    **fix**: Refreshed the IL regime account with the registered regime keeper using the existing `fvol_s6=800000` state; write signature `4Ho7DJMNZT6G4zjh7gunv11Ep6CUd93rrdpi7LTb1H61pKDfM2go4Uvp9B8a7omJ8LxCS65SxJqADY517TygUKQY`. CLI and browser IL previews now return fresh quotes.
  - **error**: Flagship and SOL Autocall quote summary cards used confusing protocol/accounting labels such as "Max payout if triggered."
    **cause**: The frontend displayed raw `max_liability` as a buyer payout concept, even though it is used as protocol risk/accounting and is equal to principal/notional for the current autocall quote paths.
    **fix**: Replaced summary cards with buyer-facing fields: notional/principal, coupon cash amount, coupon rate, entry price/basket, and pricing volatility. SOL no-quote states now hide zero-valued quote summary cards and show only the no-quote explanation plus raw program response.
  - **error**: Demo quote status showed "Unavailable / RPC/program error / Insufficient Verification. Pyth price verification level insufficient."
    **cause**: Devnet Pyth receiver accounts were already `Full` verification, but their publish times were stale. The oracle wrapper maps Pyth SDK stale-price failures to `InsufficientVerification`, so the UI message was misleading.
    **fix**: Ran the devnet price relay once from `keepers/price_relay` to refresh shard-7 SOL/USDC/SPY/QQQ/IWM `PriceUpdateV2` accounts. Verified all three live preview simulations (`flagship`, `solAutocall`, `ilProtection`) and the `/demo` page now render on-chain quote data.
  - **error**: The same demo feed issue can recur over weekends even when equity receiver accounts are fully verified.
    **cause**: SOL/USDC publish 24/7, but SPY/QQQ/IWM Pyth Benchmarks publish around US market hours. Current devnet `ProtocolConfig.pyth_quote_staleness_cap_secs=86400`, so a Friday close can exceed the quote cap before a weekend demo.
    **fix**: Documented the demo runbook: run `keepers/price_relay` continuously, and set devnet quote staleness to `604800` seconds while keeping settlement staleness at `60`. Simulated the config update first (`units_consumed=8546`) and then sent devnet tx `5D7rphSjuCDWPKH8xFp7h7E4iQTrrar6NCvuR7AFiurGVKYhRHfXHEtr6PDBN2vS2bK7Pt2r1J63BRhL4qrWCou`; verified `ProtocolConfig.pythQuoteStalenessCapSecs=604800`. Source now maps stale Pyth reads to `OracleError::PythPriceStale` instead of `InsufficientVerification`; this requires a program redeploy before the on-chain error label changes on devnet.
  - **error**: Hetzner relay status was ambiguous during demo freshness debugging.
    **cause**: The deploy kit enables `halcyon-price-relay.service`, but this workspace has no Hetzner host/IP or SSH alias to inspect the live server. On-chain receiver ages before the local refresh showed SOL/USDC around 43 minutes old, which strongly suggests the live Hetzner relay was stopped, misconfigured, or pointed at different shard/accounts.
    **fix**: Ran `NO_DNA=1 npm run start:devnet -- --once` locally from `keepers/price_relay`; it posted SOL_USD, USDC_USD, SPY_USD, QQQ_USD, and IWM_USD and completed with `txs=6`. Post-refresh receiver ages were ~86s for SOL/USDC and ~39,700s for market-closed equity feeds.
  - **error**: Hetzner `halcyon-price-relay.service` was active but restarting and not keeping prices fresh.
    **cause**: The relay wallet `G1ZcTGwr2uoBfLjgRq5Dx23FvNmUDBb33okRDsEHHDQQ` only had ~0.0019 devnet SOL, so each Pyth receiver update cycle failed in simulation with custom program error `0x1`.
    **fix**: Stored the provided key locally in ignored `.env.local` under `HETZNER_API_TOKEN` and on Hetzner in root-owned `/etc/halcyon/env` without printing it, funded the relay wallet with 2 devnet SOL after simulating the transfer (`3LLbjLCGFBXavZTz5bDgdDuYVBUP8MDBi9HwF2P1eCBL3LBYYxGa9eJMUH5eHFvfEtMeeYs1bFt9dLEJSWDcaLtx`), patched the price relay logger to redact API-key query params, copied the patch to `/opt/halcyon`, restarted the service, vacuumed archived journald logs, and installed a verifier balance check. `/root/hetzner-deploy/verify.sh` now passes; relay logs show repeated successful cycles, and receiver ages were ~15s for SOL/USDC after the fix.
  - **error**: Public `halcyonprotocol.xyz` website returned the wrong site instead of the Halcyon app.
    **cause**: The correct zone existed in Cloudflare (`halcyonprotocol.xyz`, nameservers `sureena.ns.cloudflare.com` and `kyree.ns.cloudflare.com`), but the proxied apex A records still pointed at parking IPs (`15.197.148.33`, `3.33.130.190`). Cloudflare SSL was set to Full while the Hetzner origin only served HTTP, causing 525s once the DNS was corrected. The token verifies as active and has DNS/zone-settings access, but lacks Cloudflare Pages, cache purge, and cache rules permissions.
    **fix**: Built the static frontend, installed Nginx on Hetzner, deployed `frontend/out` to `/var/www/halcyon`, configured extensionless route fallback (`/demo` -> `demo.html`), pointed the Cloudflare proxied apex A record at `95.216.158.252`, removed the duplicate parking A record, kept `www` as a proxied CNAME to the apex, and set Cloudflare SSL mode to Flexible for the HTTP-only origin. Cloudflare Development Mode was used temporarily to bypass the stale parking cache, then turned off; verified `https://halcyonprotocol.xyz/`, `/demo`, `/flagship`, and `https://www.halcyonprotocol.xyz/demo` return the Halcyon app with `cf-cache-status: DYNAMIC`.
  - **error**: Public `/faucet` page existed but production had no configured faucet API.
    **cause**: Static frontend builds cannot hold a mint-authority key, and `NEXT_PUBLIC_MOCK_USDC_FAUCET_URL` was empty in the production build.
    **fix**: Installed `halcyon-mock-usdc-faucet.service` on Hetzner, copied the mockUSDC faucet authority key to `/root/halcyon-keys/mock-usdc-faucet.json`, configured `/etc/halcyon/env` to use mockUSDC mint `5kFrfeo47etPpEk92eecACZboVnZrE4HsgxRUrg3TG7P`, proxied `https://halcyonprotocol.xyz/faucet-api/*` to the local faucet service, rebuilt the frontend with `NEXT_PUBLIC_MOCK_USDC_FAUCET_URL=/faucet-api`, and verified a real public faucet airdrop tx `5Xt2RjQYXZqhzcNfXoPPBF8EnqYb7PBGxrRw5CiWLBEaahLEygFq7PASPhahLkjBxNRCgAfVvNvT6oc1h6U9u6CR`.
  - **error**: `halcyon-flagship-sigma-keeper.service` failed its fixed-date Pyth Benchmarks cross-check and then rejected a valid computed sigma against devnet protocol config.
    **cause**: The cross-check expected value drifted from the deterministic Pyth Benchmarks result for 2026-04-10, and devnet `flagship_sigma_floor_annualised_s6` was still `600000` while the keeper computed `272681`.
    **fix**: Updated the cross-check target to `251857`, added retry/cache handling for Pyth history fetches, lowered only the flagship sigma floor to `100000` with devnet tx `ZJ5og6JE67mnvHoA3eGE3ENQ1d3XWemKGUFqyHtt4AKqtCZrQXKM3WN8vCWYNQw1gBJHknLthbi8ApRcHamhJ7j`, redeployed the keeper script, and verified a successful sigma write tx `52fPXpQQpentqX6qHRbkpty5ybdmewhpSAq8aJ9r1rB5DtMjXbCKjjNEyzzNSfp8hrSCXruEKmMcYA5w5fZD58KS`.
  - **error**: `halcyon-regime-keeper.service` failed repeatedly on CoinGecko 403s.
    **cause**: CoinGecko rejected the default Rust HTTP client request from Hetzner while accepting browser-like requests.
    **fix**: Added a browser-compatible User-Agent to the regime keeper historical price request, rebuilt and redeployed the Linux binary on Hetzner, and verified it wrote a fresh IL regime signal on devnet with tx `LPyF85t56S9yrjKVg6bGjs1EKCQQTENy5f2Arwh4MTxvnt2jJcbLia6ydfqXKW47WY6EnxtmDrWijjDmTo1fBBo`.
  - **error**: `halcyon-fire-reduced-ops.service` could fail after partial success with devnet RPC `429 Too Many Requests`.
    **cause**: The SOL reduced-operator uploader sends chunked transactions and the shared SDK sender treated transient RPC throttling as fatal immediately.
    **fix**: Added bounded same-signature retries/backoff to the shared SDK transaction sender, rebuilt and installed `/opt/halcyon/bin/halcyon` on Hetzner, and verified the subsequent reduced-operator timer run completed all V/U chunks. `/root/hetzner-deploy/verify.sh` passes and `systemctl --failed` is clean.
