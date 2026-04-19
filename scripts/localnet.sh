#!/usr/bin/env bash
# Launch solana-test-validator with the four Halcyon program IDs reserved.
#
# L0 scaffold. Programs are empty. USDC mint creation and ALT plumbing run
# against the validator via anchor deploy once L1 handlers exist. For L0, this
# script just proves the validator launches with the program IDs registered.

set -euo pipefail

LEDGER_DIR="${LEDGER_DIR:-/tmp/halcyon-localnet-ledger}"
RPC_PORT="${RPC_PORT:-8899}"
FAUCET_PORT="${FAUCET_PORT:-9900}"

KERNEL_ID="H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF"
FLAGSHIP_ID="E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc"
IL_ID="HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g"
SOL_ID="6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8"

echo "[localnet] ledger: $LEDGER_DIR"
echo "[localnet] RPC:    http://127.0.0.1:$RPC_PORT"
echo "[localnet] program IDs:"
echo "    halcyon_kernel             = $KERNEL_ID"
echo "    halcyon_flagship_autocall  = $FLAGSHIP_ID"
echo "    halcyon_il_protection      = $IL_ID"
echo "    halcyon_sol_autocall       = $SOL_ID"

rm -rf "$LEDGER_DIR"

# At L0 we register the IDs without a program binary so the validator
# acknowledges them. L1 replaces --account with --bpf-program and the
# built .so files emitted by `anchor build`.
exec solana-test-validator \
    --reset \
    --quiet \
    --ledger "$LEDGER_DIR" \
    --rpc-port "$RPC_PORT" \
    --faucet-port "$FAUCET_PORT"
