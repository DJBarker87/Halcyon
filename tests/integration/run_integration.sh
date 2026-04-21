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

yarn tsc -p tsconfig.json --outDir "$COMPILED_DIR" >/dev/null
node "$COMPILED_DIR/tests/integration/mock_pyth.js" \
  --write-fixtures "$FIXTURES_DIR" \
  --manifest "$MANIFEST_PATH" \
  --base-ts "$(date +%s)" >/dev/null

NO_DNA=1 anchor build --skip-lint --no-idl -- --no-default-features --features integration-test

VALIDATOR_ARGS=(
  --reset
  --quiet
  --ledger "$LEDGER_DIR"
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

HALCYON_MOCK_PYTH_MANIFEST="$MANIFEST_PATH" \
anchor test \
  --skip-build \
  --skip-local-validator \
  --run tests/integration/real_products.spec.ts

popd >/dev/null
