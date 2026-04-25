#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT_DIR/.anchor/integration"
COMPILED_DIR="$OUT_DIR/compiled"
FIXTURES_DIR="$OUT_DIR/fixtures"
LEDGER_DIR="$OUT_DIR/ledger"
MANIFEST_PATH="$OUT_DIR/mock_pyth_manifest.json"
VALIDATOR_LOG="$OUT_DIR/validator.log"

mkdir -p "$OUT_DIR"
rm -rf "$COMPILED_DIR" "$FIXTURES_DIR" "$LEDGER_DIR"

pushd "$ROOT_DIR" >/dev/null

for manifest in \
  "$ROOT_DIR/programs/halcyon_kernel/Cargo.toml" \
  "$ROOT_DIR/programs/halcyon_flagship_autocall/Cargo.toml" \
  "$ROOT_DIR/programs/halcyon_il_protection/Cargo.toml" \
  "$ROOT_DIR/programs/halcyon_sol_autocall/Cargo.toml"
do
  features="integration-test"
  if [[ "$manifest" == "$ROOT_DIR/programs/halcyon_flagship_autocall/Cargo.toml" && -n "${FLAGSHIP_EXTRA_FEATURES:-}" ]]; then
    features="$features,$FLAGSHIP_EXTRA_FEATURES"
  fi
  if [[ "$manifest" == "$ROOT_DIR/programs/halcyon_sol_autocall/Cargo.toml" && -n "${SOL_EXTRA_FEATURES:-}" ]]; then
    features="$features,$SOL_EXTRA_FEATURES"
  fi
  NO_DNA=1 cargo build-sbf \
    --manifest-path "$manifest" \
    --sbf-out-dir "$ROOT_DIR/target/deploy" \
    --no-default-features \
    --features "$features"
done

./node_modules/.bin/tsc \
  tests/integration/mock_pyth.ts \
  --outDir "$COMPILED_DIR" \
  --module commonjs \
  --target es2020 \
  --esModuleInterop \
  --skipLibCheck >/dev/null

node "$COMPILED_DIR/mock_pyth.js" \
  --write-fixtures "$FIXTURES_DIR" \
  --manifest "$MANIFEST_PATH" \
  --base-ts "$(date +%s)" >/dev/null

VALIDATOR_ARGS=(
  --reset
  --quiet
  --ledger "$LEDGER_DIR"
  --upgradeable-program H71FxCTuVGL13PkzXeVxeTn89xZreFm4AwLu3iZeVtdF "$ROOT_DIR/target/deploy/halcyon_kernel.so" "${ANCHOR_WALLET:-$HOME/.config/solana/id.json}"
  --upgradeable-program E4Atu2kHkzJ1NMATBvoMcy3BDKfsyz418DHCoqQHc3Mc "$ROOT_DIR/target/deploy/halcyon_flagship_autocall.so" "${ANCHOR_WALLET:-$HOME/.config/solana/id.json}"
  --upgradeable-program HuUQUngf79HgTWdggxAsE135qFeHfYV9Mj9xsCcwqz5g "$ROOT_DIR/target/deploy/halcyon_il_protection.so" "${ANCHOR_WALLET:-$HOME/.config/solana/id.json}"
  --upgradeable-program 6DfpE7MEx1K1CeiQuw8Q61Empamcuknv9Tc79xtJKae8 "$ROOT_DIR/target/deploy/halcyon_sol_autocall.so" "${ANCHOR_WALLET:-$HOME/.config/solana/id.json}"
)

for fixture in "$FIXTURES_DIR"/*.json; do
  pubkey="$(basename "$fixture" .json)"
  VALIDATOR_ARGS+=(--account "$pubkey" "$fixture")
done

solana-test-validator "${VALIDATOR_ARGS[@]}" >"$VALIDATOR_LOG" 2>&1 &
VALIDATOR_PID=$!

cleanup() {
  if kill -0 "$VALIDATOR_PID" >/dev/null 2>&1; then
    kill "$VALIDATOR_PID" >/dev/null 2>&1 || true
    wait "$VALIDATOR_PID" >/dev/null 2>&1 || true
  fi
}

trap cleanup EXIT

for _ in $(seq 1 60); do
  if solana --url http://127.0.0.1:8899 cluster-version >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! solana --url http://127.0.0.1:8899 cluster-version >/dev/null 2>&1; then
  echo "local validator did not start; see $VALIDATOR_LOG" >&2
  exit 1
fi

if [[ -n "${MIDLIFE_INTEGRATION_SPECS:-}" ]]; then
  read -r -a INTEGRATION_SPECS <<<"$MIDLIFE_INTEGRATION_SPECS"
else
  INTEGRATION_SPECS=(
    tests/integration/real_products.spec.ts
    tests/integration/midlife_parity.spec.ts
  )
fi

HALCYON_MOCK_PYTH_MANIFEST="$MANIFEST_PATH" \
ANCHOR_PROVIDER_URL="http://127.0.0.1:8899" \
ANCHOR_WALLET="${ANCHOR_WALLET:-$HOME/.config/solana/id.json}" \
./node_modules/.bin/ts-mocha \
  -p ./tsconfig.json \
  -t "${INTEGRATION_MOCHA_TIMEOUT_MS:-3600000}" \
  "${INTEGRATION_SPECS[@]}"

popd >/dev/null
