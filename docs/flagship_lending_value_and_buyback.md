# Flagship Lending Value And Buyback

This document maps the thesis-pivot collateral flow to the repo implementation.

## Mechanism

For an active Flagship policy, the program computes a fresh mid-life NAV from
the same on-chain state used by settlement and preview paths. Lending value is:

```text
lending_value_s6 = max(0, min(current_nav_s6 - 100000, ki_level_s6 - 100000))
payout_usdc = policy_notional_usdc * lending_value_s6 / 1000000
```

The 100000 scale-6 haircut is 10% of notional. The healthy-state cap at
`ki_level - 10%` gives lenders a conservative liquidation ceiling, while stress
states follow current NAV down with the same haircut.

Retail redemption uses the same cap shape with a 5% scale-6 haircut, but only
after a 48-hour notice period:

```text
retail_redemption_value_s6 = max(0, min(current_nav_s6 - 50000, ki_level_s6 - 50000))
```

## On-Chain Surface

Flagship program:

- `preview_lending_value`: returns NAV, KI level, lending value, lending-value
  payout, remaining coupon PV, par-recovery probability, pricing sigma, and the
  trading-day index used by the mid-life pricer.
- `buyback`: recomputes the same NAV in-transaction, settles the policy with
  `SettlementReason::Buyback`, pays the current owner ATA from the kernel vault,
  marks the product terms settled, and emits `FlagshipBuybackExecuted`.
- `request_retail_redemption`: starts a 48-hour notice window for the tighter
  retail exit.
- `cancel_retail_redemption`: closes an outstanding notice request.
- `execute_retail_redemption`: after the notice window, recomputes NAV
  in-transaction, applies the 5% retail haircut, settles with
  `SettlementReason::RetailRedemption`, and closes the request.

Kernel program:

- `transfer_policy_owner`: moves an active `PolicyHeader.owner` from the current
  owner to another wallet or program PDA, emitting `PolicyOwnerTransferred`.
- `wrap_policy_receipt`: moves an active `PolicyHeader.owner` into a kernel
  receipt-authority PDA and mints a 1-supply SPL receipt token to the holder's
  ATA.
- `unwrap_policy_receipt`: burns the holder's receipt token and restores direct
  policy ownership to the token holder.

The direct owner transfer is the program-escrow path. The SPL receipt is the
wallet-visible collateral path.

Client SDK and CLI:

- `halcyon preview-lending-value <POLICY> --pyth-spy ... --pyth-qqq ... --pyth-iwm ...`
- `halcyon transfer-policy-owner <POLICY> --new-owner <ESCROW_OR_WALLET>`
- `halcyon buyback-flagship <POLICY> --pyth-spy ... --pyth-qqq ... --pyth-iwm ...`
- `halcyon wrap-policy-receipt <POLICY>`
- `halcyon unwrap-policy-receipt <POLICY>`
- `halcyon request-retail-redemption <POLICY>`
- `halcyon execute-retail-redemption <POLICY> --pyth-spy ... --pyth-qqq ... --pyth-iwm ...`
- `halcyon liquidate-wrapped-flagship <POLICY> --pyth-spy ... --pyth-qqq ... --pyth-iwm ...`

`--usdc-mint` remains available as an override. Devnet ops should instead set
`USDC_MINT` or `HALCYON_USDC_MINT` to the mock-USDC mint used by the faucet.

Frontend:

- The portfolio table simulates `preview_lending_value` for active Flagship
  policies and displays lending value, NAV, and KI level when the configured
  cluster has the required program, IDL, and Pyth feed accounts.
- `/lending-demo` is a fake integration site. It shows receipt-token collateral
  rows, a demo fallback book, live wallet policy loading, tokenization, and a
  liquidate button that builds unwrap-plus-buyback instructions for wrapped
  Flagship receipts.

## Reference Consumer

`research/programs/halcyon_lending_consumer` is a minimal Anchor program showing
the lending-protocol side of the flow:

1. A user or lending protocol transfers the policy owner field to the consumer's
   escrow PDA: `["policy_escrow", policy_header]`.
2. On liquidation, the consumer verifies that the escrow PDA is the current
   owner.
3. The consumer signs for that PDA and CPI-calls `halcyon_flagship_autocall::buyback`.
4. The flagship program pays the escrow PDA's USDC ATA and settles the note.

This is deliberately not a full lending market. It proves the technical
property the thesis needs: a third-party protocol can hold the note and close
the seized collateral through the issuer's deterministic on-chain exit.

## Validation

The mechanism-active flagship replay is stored in
`research/flagship_buyback_outputs/`.

- Production daily-issuance row: 4,291 issued notes.
- Primary lending-style liquidation: 452 liquidations, 0 failures, 1.3319x
  minimum coverage.
- Forced stress liquidation: 708 liquidations, 0 failures, 1.2458x minimum
  coverage.

The result validates per-note solvency under the current research hedge-unwind
model. It does not eliminate wrapper-liquidity risk; that remains a production
calibration task for the tokenised-equity venue actually used at launch.
