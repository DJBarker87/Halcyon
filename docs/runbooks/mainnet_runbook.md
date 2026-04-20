# Mainnet Runbook

This is the Layer 5 launch sequence for Halcyon mainnet.

## 1. Preconditions

- audit freeze is in effect for all four programs
- multisig signers are available
- keeper hosts are provisioned
- mainnet RPC primary and failover endpoints are configured
- frontend mainnet environment values are reviewed
- monitoring and alerting stack is live

## 2. Generate and store launch materials

Create fresh mainnet keys for:

- observation keeper
- regression keeper
- delta keeper
- hedge keeper
- regime keeper

Store:

- key custody owner
- hardware location
- public key
- last verification timestamp

Do not reuse devnet keeper keys.

## 3. Multisig ceremony

Before any public launch steps:

1. transfer or confirm `ProtocolConfig.admin` under the chosen multisig
2. rehearse one harmless admin instruction through multisig UX
3. archive the successful transaction signature

Recommended keeper role IDs for `rotate-keeper`:

- `0` observation
- `1` regression
- `2` delta
- `3` hedge
- `4` regime

## 4. Deploy and register programs

Deploy:

- `halcyon_kernel`
- `halcyon_sol_autocall`
- `halcyon_il_protection`
- `halcyon_flagship_autocall`

Then register products and kernel state with the operator CLI.

Illustrative commands:

```bash
cargo run -p halcyon_cli -- \
  --rpc "$RPC_URL" \
  --keypair "$ADMIN_KEYPAIR" \
  init-protocol \
  --usdc-mint "$USDC_MINT"

cargo run -p halcyon_cli -- \
  --rpc "$RPC_URL" \
  --keypair "$ADMIN_KEYPAIR" \
  register-sol-autocall

cargo run -p halcyon_cli -- \
  --rpc "$RPC_URL" \
  --keypair "$ADMIN_KEYPAIR" \
  register-il-protection
```

Flagship registration follows the same product-registry path if the mainnet rollout intends to deploy it in paused state.

## 5. Register lookup tables

For each live product:

1. create the product ALT
2. extend it with the product/kernel account set
3. register it in the kernel lookup-table registry
4. verify the frontend can read it from the selected RPC

Do not open issuance until ALT registry reads succeed.

## 6. Rotate keeper authorities

Rotate each keeper role to the fresh mainnet authority:

```bash
cargo run -p halcyon_cli -- \
  --rpc "$RPC_URL" \
  --keypair "$ADMIN_KEYPAIR" \
  rotate-keeper \
  --role 3 \
  --new-authority "$HEDGE_KEEPER_PUBKEY"
```

Repeat for each role.

## 7. Seed launch capital

Before opening issuance:

- seed junior capital
- fund any coupon vault or hedge sleeve balances required by the active products

Archive transaction signatures for:

- junior seed
- coupon-vault funding
- hedge-sleeve funding

## 8. Pause drill on live deployment

Immediately after deploy and before external issuance:

1. set issuance paused
2. attempt issuance and confirm rejection
3. unpause
4. perform one smoke issuance successfully

No external user should see this drill.

## 9. Product activation policy

Launch default:

- SOL Autocall: active
- IL Protection: active
- Flagship: deployed, registered, paused unless explicit go-live signoff exists

If flagship legal or liquidity signoff is incomplete, keep it paused.

## 10. Browser smoke

Using the Layer 5 frontend:

1. load the mainnet runtime config
2. connect supported wallet
3. preview quote on each intended live product
4. issue one operator-owned smoke policy at low notional
5. confirm portfolio and vault pages reflect new state

## 11. Post-deploy evidence bundle

Archive:

- deploy transaction signatures
- multisig transaction signature proving admin control
- keeper rotation transaction signatures
- pause drill transaction signatures
- first smoke issuance signatures
- screenshots of green monitoring dashboards

## 12. Stop conditions

Abort launch or re-pause issuance if any of the following occur:

- keeper heartbeat alert fires during launch sequence
- any required Pyth feed exceeds its staleness cap
- ALT registry lookup fails from production RPC
- first smoke issuance or settlement path fails
- vault utilization is unexpectedly elevated before public issuance
