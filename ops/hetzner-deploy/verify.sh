#!/usr/bin/env bash
set -euo pipefail

ENV_FILE="/etc/halcyon/env"
FAILURES=0

load_env() {
  if [[ ! -f "${ENV_FILE}" ]]; then
    echo "missing ${ENV_FILE}" >&2
    exit 1
  fi
  set -a
  # shellcheck disable=SC1090
  source "${ENV_FILE}"
  set +a
}

is_enabled() {
  case "${1:-0}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

check_unit() {
  local unit="$1"
  systemctl status --no-pager --lines=0 "${unit}" >/dev/null 2>&1 || true
  if systemctl is-active --quiet "${unit}"; then
    echo "PASS unit ${unit} is active"
  else
    echo "FAIL unit ${unit} is not active"
    FAILURES=$((FAILURES + 1))
  fi
}

check_required_env() {
  local name="$1"
  if [[ -n "${!name:-}" ]]; then
    echo "PASS env ${name} is set"
  else
    echo "FAIL env ${name} is missing"
    FAILURES=$((FAILURES + 1))
  fi
}

check_key_history() {
  local label="$1"
  local keypair_path="$2"

  if [[ ! -f "${keypair_path}" ]]; then
    echo "FAIL key ${label} missing at ${keypair_path}"
    FAILURES=$((FAILURES + 1))
    return
  fi

  local pubkey
  pubkey="$(solana address -k "${keypair_path}")"
  local output
  output="$(solana transaction-history "${pubkey}" --limit 5 --url "${HELIUS_DEVNET_RPC}" 2>&1 || true)"

  if grep -qiE "No confirmed transactions found|No transaction history found|No transaction history" <<<"${output}"; then
    echo "FAIL key ${label} (${pubkey}) has no recent transaction history"
    FAILURES=$((FAILURES + 1))
  else
    echo "PASS key ${label} (${pubkey}) has transaction history"
  fi
}

check_sol_balance_min() {
  local label="$1"
  local keypair_path="$2"
  local min_sol="$3"

  if [[ ! -f "${keypair_path}" ]]; then
    echo "FAIL balance ${label} key missing at ${keypair_path}"
    FAILURES=$((FAILURES + 1))
    return
  fi

  local pubkey
  pubkey="$(solana address -k "${keypair_path}")"
  local output
  output="$(solana balance -k "${keypair_path}" --url "${HELIUS_DEVNET_RPC}" 2>&1 || true)"
  local balance_sol
  balance_sol="$(awk '{print $1}' <<<"${output}")"

  if python3 - "${balance_sol}" "${min_sol}" <<'PY'
import sys
try:
    balance = float(sys.argv[1])
    minimum = float(sys.argv[2])
except Exception:
    sys.exit(2)
sys.exit(0 if balance >= minimum else 1)
PY
  then
    echo "PASS balance ${label} (${pubkey}) ${balance_sol} SOL >= ${min_sol} SOL"
  else
    echo "FAIL balance ${label} (${pubkey}) ${balance_sol:-<unknown>} SOL < ${min_sol} SOL"
    FAILURES=$((FAILURES + 1))
  fi
}

main() {
  load_env

  if is_enabled "${ENABLE_PRICE_RELAY:-1}"; then
    check_unit halcyon-price-relay.service
    check_key_history price-relay "${PRICE_RELAY_KEYPAIR}"
    check_sol_balance_min price-relay "${PRICE_RELAY_KEYPAIR}" "${PRICE_RELAY_MIN_BALANCE_SOL:-0.25}"
  fi
  if is_enabled "${ENABLE_OBSERVATION_KEEPER:-1}"; then
    check_unit halcyon-observation-keeper.service
    check_key_history observation "${OBSERVATION_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_REGIME_KEEPER:-1}"; then
    check_unit halcyon-regime-keeper.service
    check_key_history regime "${REGIME_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_IL_EWMA_TIMER:-1}"; then
    check_unit halcyon-update-ewma-il.timer
    check_key_history il-ewma "${IL_EWMA_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_SOL_EWMA_TIMER:-1}"; then
    check_unit halcyon-update-ewma-sol.timer
    check_key_history sol-ewma "${SOL_EWMA_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_FLAGSHIP_EWMA_TIMER:-0}"; then
    check_unit halcyon-update-ewma-flagship.timer
    check_key_history flagship-ewma "${FLAGSHIP_EWMA_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_FLAGSHIP_SIGMA_TIMER:-1}"; then
    check_unit halcyon-flagship-sigma-keeper.timer
    check_key_history flagship-sigma "${FLAGSHIP_SIGMA_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_REDUCED_OPS_TIMER:-1}"; then
    check_unit halcyon-fire-reduced-ops.timer
    check_key_history reduced-ops-regime "${REGIME_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_AUTOCALL_SCHEDULE_TIMER:-1}"; then
    check_unit halcyon-write-autocall-schedule.timer
    check_key_history autocall-schedule-observation "${OBSERVATION_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_WRITE_REGRESSION_TIMER:-0}"; then
    check_unit halcyon-write-regression.timer
    check_key_history write-regression "${REGRESSION_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_DELTA_KEEPER:-0}"; then
    check_required_env PINATA_JWT
    check_unit halcyon-delta-keeper.service
    check_key_history delta "${DELTA_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_HEDGE_KEEPER:-0}"; then
    check_unit halcyon-hedge-keeper.service
    check_key_history hedge "${HEDGE_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_LEGACY_REGRESSION_KEEPER:-0}"; then
    check_unit halcyon-regression-keeper.service
    check_key_history legacy-regression "${REGRESSION_KEYPAIR}"
  fi
  if is_enabled "${ENABLE_FLAGSHIP_HEDGE_KEEPER:-0}"; then
    check_unit halcyon-flagship-hedge-keeper.service
    check_key_history flagship-hedge "${FLAGSHIP_HEDGE_KEYPAIR}"
  fi

  if [[ "${FAILURES}" -gt 0 ]]; then
    echo "Verification finished with ${FAILURES} failure(s)."
    exit 1
  fi

  echo "Verification passed."
}

main "$@"
