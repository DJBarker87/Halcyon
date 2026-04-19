# Halcyon kernel — account layouts

Hand-authored byte-layout reference for every kernel-owned account.
Keepers and the frontend build against the IDL; humans read this.
`make layouts-check` parses `target/idl/halcyon_kernel.json` and asserts the
two agree.

All accounts are prefixed by Anchor's 8-byte discriminator. Sizes below are the
**payload** — add 8 for on-chain space.

Primitive sizes (Borsh): `u8/i8 = 1`, `u16/i16 = 2`, `u32/i32 = 4`,
`u64/i64 = 8`, `u128/i128 = 16`, `Pubkey = 32`, `bool = 1`, `[u8; N] = N`.
Fixed-size arrays add no length prefix. Borsh enums use 1 byte for the
discriminator.

Every struct has `u8 version` at offset 0 for in-place upgrade migration.

---

## ProtocolConfig — singleton

| Field                            | Type     | Bytes | Offset |
|----------------------------------|----------|-------|--------|
| version                          | u8       | 1     | 0      |
| admin                            | Pubkey   | 32    | 1      |
| issuance_paused_global           | bool     | 1     | 33     |
| settlement_paused_global         | bool     | 1     | 34     |
| utilization_cap_bps              | u64      | 8     | 35     |
| senior_share_bps                 | u16      | 2     | 43     |
| junior_share_bps                 | u16      | 2     | 45     |
| treasury_share_bps               | u16      | 2     | 47     |
| senior_cooldown_secs             | i64      | 8     | 49     |
| ewma_rate_limit_secs             | i64      | 8     | 57     |
| sigma_staleness_cap_secs         | i64      | 8     | 65     |
| regime_staleness_cap_secs        | i64      | 8     | 73     |
| regression_staleness_cap_secs    | i64      | 8     | 81     |
| pyth_quote_staleness_cap_secs    | i64      | 8     | 89     |
| pyth_settle_staleness_cap_secs   | i64      | 8     | 97     |
| quote_ttl_secs                   | i64      | 8     | 105    |
| sigma_floor_annualised_s6        | i64      | 8     | 113    |
| sol_autocall_quote_share_bps     | u16      | 2     | 121    |
| sol_autocall_issuer_margin_bps   | u16      | 2     | 123    |
| k12_correction_sha256            | [u8; 32] | 32    | 125    |
| daily_ki_correction_sha256       | [u8; 32] | 32    | 157    |
| treasury_destination             | Pubkey   | 32    | 189    |
| last_update_ts                   | i64      | 8     | 221    |
| **TOTAL**                        |          | **229** |      |

`treasury_destination` is the only USDC account `sweep_fees` is allowed to
route to — K5 allowlist. Admin rotates via `set_protocol_config`; each
rotation emits `ConfigUpdated` so a compromised admin cannot exfiltrate in
one observable state change.

## ProductRegistryEntry — one per registered product

| Field                     | Type    | Bytes | Offset |
|---------------------------|---------|-------|--------|
| version                   | u8      | 1     | 0      |
| product_program_id        | Pubkey  | 32    | 1      |
| expected_authority        | Pubkey  | 32    | 33     |
| active                    | bool    | 1     | 65     |
| paused                    | bool    | 1     | 66     |
| per_policy_risk_cap       | u64     | 8     | 67     |
| global_risk_cap           | u64     | 8     | 75     |
| engine_version            | u16     | 2     | 83     |
| init_terms_discriminator  | [u8; 8] | 8     | 85     |
| total_reserved            | u64     | 8     | 93     |
| last_update_ts            | i64     | 8     | 101    |
| **TOTAL**                 |         | **109** |      |

`total_reserved` is the running per-product sum of `max_liability` across
Quoted-or-Active policies. `reserve_and_issue` increments, `apply_settlement`
and `reap_quoted` decrement. Gates `global_risk_cap` — K9 fix.

## VaultState — singleton

| Field                      | Type | Bytes | Offset |
|----------------------------|------|-------|--------|
| version                    | u8   | 1     | 0      |
| total_senior               | u64  | 8     | 1      |
| total_junior               | u64  | 8     | 9      |
| total_reserved_liability   | u64  | 8     | 17     |
| lifetime_premium_received  | u64  | 8     | 25     |
| last_update_slot           | u64  | 8     | 33     |
| last_update_ts             | i64  | 8     | 41     |
| **TOTAL**                  |      | **49** |      |

## SeniorDeposit — one per senior depositor

| Field             | Type   | Bytes | Offset |
|-------------------|--------|-------|--------|
| version           | u8     | 1     | 0      |
| owner             | Pubkey | 32    | 1      |
| balance           | u64    | 8     | 33     |
| accrued_yield     | u64    | 8     | 41     |
| last_deposit_ts   | i64    | 8     | 49     |
| created_ts        | i64    | 8     | 57     |
| **TOTAL**         |        | **65** |      |

## JuniorTranche — one per junior depositor

| Field               | Type   | Bytes | Offset |
|---------------------|--------|-------|--------|
| version             | u8     | 1     | 0      |
| owner               | Pubkey | 32    | 1      |
| balance             | u64    | 8     | 33     |
| non_withdrawable    | bool   | 1     | 41     |
| created_ts          | i64    | 8     | 42     |
| **TOTAL**           |        | **50** |      |

## PolicyHeader — one per live policy

| Field               | Type          | Bytes | Offset |
|---------------------|---------------|-------|--------|
| version             | u8            | 1     | 0      |
| product_program_id  | Pubkey        | 32    | 1      |
| owner               | Pubkey        | 32    | 33     |
| notional            | u64           | 8     | 65     |
| premium_paid        | u64           | 8     | 73     |
| max_liability       | u64           | 8     | 81     |
| issued_at           | i64           | 8     | 89     |
| expiry_ts           | i64           | 8     | 97     |
| quote_expiry_ts     | i64           | 8     | 105    |
| settled_at          | i64           | 8     | 113    |
| terms_hash          | [u8; 32]      | 32    | 121    |
| engine_version      | u16           | 2     | 153    |
| status              | u8 (enum tag) | 1     | 155    |
| product_terms       | Pubkey        | 32    | 156    |
| shard_id            | u16           | 2     | 188    |
| policy_id           | Pubkey        | 32    | 190    |
| **TOTAL**           |               | **222** |      |

`status` is a Borsh enum serialized as a single tag byte (variants are
`Quoted=0`, `Active=1`, `Observed=2`, `AutoCalled=3`, `KnockedIn=4`,
`Settled=5`, `Expired=6`, `Cancelled=7`).

## CouponVault — one per autocall product

| Field                    | Type   | Bytes | Offset |
|--------------------------|--------|-------|--------|
| version                  | u8     | 1     | 0      |
| product_program_id       | Pubkey | 32    | 1      |
| usdc_balance             | u64    | 8     | 33     |
| lifetime_coupons_paid    | u64    | 8     | 41     |
| last_update_ts           | i64    | 8     | 49     |
| **TOTAL**                |        | **57** |      |

## HedgeSleeve — one per hedged product

| Field                     | Type   | Bytes | Offset |
|---------------------------|--------|-------|--------|
| version                   | u8     | 1     | 0      |
| product_program_id        | Pubkey | 32    | 1      |
| usdc_reserve              | u64    | 8     | 33     |
| cumulative_funded_usdc    | u64    | 8     | 41     |
| cumulative_defunded_usdc  | u64    | 8     | 49     |
| lifetime_execution_cost   | u64    | 8     | 57     |
| last_funded_ts            | i64    | 8     | 65     |
| last_defunded_ts          | i64    | 8     | 73     |
| last_update_ts            | i64    | 8     | 81     |
| **TOTAL**                 |        | **89** |      |

## HedgeBookState — one per hedged product (4 legs max)

Nested `HedgeLeg`: `asset_tag [u8;8] + current_position_raw i64 +
target_position_raw i64 + last_rebalance_ts i64 + last_rebalance_price_s6 i64`
= 40 bytes per leg.

| Field                             | Type                  | Bytes | Offset |
|-----------------------------------|-----------------------|-------|--------|
| version                           | u8                    | 1     | 0      |
| product_program_id                | Pubkey                | 32    | 1      |
| leg_count                         | u8                    | 1     | 33     |
| legs                              | [HedgeLeg; 4]         | 160   | 34     |
| last_aggregate_delta_spot_s6      | [i64; 4]              | 32    | 194    |
| cumulative_execution_cost         | u64                   | 8     | 226    |
| last_rebalance_ts                 | i64                   | 8     | 234    |
| sequence                          | u64                   | 8     | 242    |
| **TOTAL**                         |                       | **250** |      |

`sequence` is a monotonic counter written by `record_hedge_trade` — every
trade must pass `args.sequence > hedge_book.sequence`. Replays and reordered
keeper submissions are rejected by the kernel (K4 fix).

## PendingHedgeSwap — one per hedged product

Transient escrow of the keeper-approved swap envelope. `prepare_hedge_swap`
writes it, Jupiter executes in the same transaction, and `record_hedge_trade`
consumes and clears it.

| Field                        | Type     | Bytes | Offset |
|-----------------------------|----------|-------|--------|
| version                     | u8       | 1     | 0      |
| active                      | bool     | 1     | 1      |
| product_program_id          | Pubkey   | 32    | 2      |
| keeper                      | Pubkey   | 32    | 34     |
| asset_tag                   | [u8; 8]  | 8     | 66     |
| leg_index                   | u8       | 1     | 74     |
| source_is_wsol              | bool     | 1     | 75     |
| old_position_raw            | i64      | 8     | 76     |
| target_position_raw         | i64      | 8     | 84     |
| min_position_raw            | i64      | 8     | 92     |
| max_position_raw            | i64      | 8     | 100    |
| approved_input_amount       | u64      | 8     | 108    |
| source_balance_before       | u64      | 8     | 116    |
| destination_balance_before  | u64      | 8     | 124    |
| spot_price_s6               | i64      | 8     | 132    |
| max_slippage_bps            | u16      | 2     | 140    |
| sequence                    | u64      | 8     | 142    |
| prepared_at                 | i64      | 8     | 150    |
| **TOTAL**                   |          | **158** |      |

## AggregateDelta — flagship only

| Field                 | Type     | Bytes | Offset |
|-----------------------|----------|-------|--------|
| version               | u8       | 1     | 0      |
| product_program_id    | Pubkey   | 32    | 1      |
| delta_spy_s6          | i64      | 8     | 33     |
| delta_qqq_s6          | i64      | 8     | 41     |
| delta_iwm_s6          | i64      | 8     | 49     |
| merkle_root           | [u8; 32] | 32    | 57     |
| spot_spy_s6           | i64      | 8     | 89     |
| spot_qqq_s6           | i64      | 8     | 97     |
| spot_iwm_s6           | i64      | 8     | 105    |
| live_note_count       | u32      | 4     | 113    |
| last_update_slot      | u64      | 8     | 117    |
| last_update_ts        | i64      | 8     | 125    |
| **TOTAL**             |          | **133** |      |

## Regression — flagship only

| Field              | Type | Bytes | Offset |
|--------------------|------|-------|--------|
| version            | u8   | 1     | 0      |
| beta_spy_s12       | i128 | 16    | 1      |
| beta_qqq_s12       | i128 | 16    | 17     |
| alpha_s12          | i128 | 16    | 33     |
| r_squared_s6       | i64  | 8     | 49     |
| residual_vol_s6    | i64  | 8     | 57     |
| window_start_ts    | i64  | 8     | 65     |
| window_end_ts      | i64  | 8     | 73     |
| last_update_slot   | u64  | 8     | 81     |
| last_update_ts     | i64  | 8     | 89     |
| sample_count       | u32  | 4     | 97     |
| **TOTAL**          |      | **101** |      |

## VaultSigma — one per product

| Field                        | Type   | Bytes | Offset |
|------------------------------|--------|-------|--------|
| version                      | u8     | 1     | 0      |
| product_program_id           | Pubkey | 32    | 1      |
| oracle_feed_id               | [u8; 32] | 32  | 33     |
| ewma_var_daily_s12           | i128   | 16    | 65     |
| ewma_last_ln_ratio_s12       | i128   | 16    | 81     |
| ewma_last_timestamp          | i64    | 8     | 97     |
| last_price_s6                | i64    | 8     | 105    |
| last_publish_ts              | i64    | 8     | 113    |
| last_publish_slot            | u64    | 8     | 121    |
| last_update_slot             | u64    | 8     | 129    |
| sample_count                 | u64    | 8     | 137    |
| **TOTAL**                    |        | **145** |      |

## RegimeSignal — one per product using regime-switching

| Field                          | Type          | Bytes | Offset |
|--------------------------------|---------------|-------|--------|
| version                        | u8            | 1     | 0      |
| product_program_id             | Pubkey        | 32    | 1      |
| fvol_s6                        | i64           | 8     | 33     |
| regime                         | u8 (enum tag) | 1     | 41     |
| sigma_multiplier_s6            | i64           | 8     | 42     |
| sigma_floor_annualised_s6      | i64           | 8     | 50     |
| last_update_ts                 | i64           | 8     | 58     |
| last_update_slot               | u64           | 8     | 66     |
| **TOTAL**                      |               | **74** |      |

`regime`: `Calm=0`, `Stress=1`.

## FeeLedger — singleton

Nested `FeeBucket`: `product_program_id Pubkey + accrued_usdc u64` = 40 bytes
per bucket.

| Field              | Type              | Bytes | Offset |
|--------------------|-------------------|-------|--------|
| version            | u8                | 1     | 0      |
| treasury_balance   | u64               | 8     | 1      |
| bucket_count       | u8                | 1     | 9      |
| buckets            | [FeeBucket; 8]    | 320   | 10     |
| last_sweep_ts      | i64               | 8     | 330    |
| **TOTAL**          |                   | **338** |      |

## KeeperRegistry — singleton

| Field              | Type   | Bytes | Offset |
|--------------------|--------|-------|--------|
| version            | u8     | 1     | 0      |
| observation       | Pubkey | 32    | 1      |
| regression        | Pubkey | 32    | 33     |
| delta             | Pubkey | 32    | 65     |
| hedge             | Pubkey | 32    | 97     |
| regime            | Pubkey | 32    | 129    |
| last_rotation_ts  | i64    | 8     | 161    |
| **TOTAL**         |        | **169** |      |

## LookupTableRegistry — per-product (4 tables max)

| Field                  | Type        | Bytes | Offset |
|------------------------|-------------|-------|--------|
| version                | u8          | 1     | 0      |
| product_program_id     | Pubkey      | 32    | 1      |
| count                  | u8          | 1     | 33     |
| tables                 | [Pubkey; 4] | 128   | 34     |
| last_update_ts         | i64         | 8     | 162    |
| **TOTAL**              |             | **170** |      |
