#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="/etc/halcyon/env"
HALCYON_DIR="/opt/halcyon"
BIN_DIR="${HALCYON_DIR}/bin"
CONFIG_DIR="/etc/halcyon/config"
CALIBRATION_DIR="/etc/halcyon/calibration"
PYTHON_VENV_DIR="${HALCYON_DIR}/.venv"

require_root() {
  if [[ "$(id -u)" -ne 0 ]]; then
    echo "deploy.sh must run as root" >&2
    exit 1
  fi
}

load_env() {
  if [[ ! -f "${ENV_FILE}" ]]; then
    echo "missing ${ENV_FILE}; copy ops/hetzner-deploy/env.example there first" >&2
    exit 1
  fi
  set -a
  # shellcheck disable=SC1090
  source "${ENV_FILE}"
  set +a
}

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "required env var ${name} is missing in ${ENV_FILE}" >&2
    exit 1
  fi
}

is_enabled() {
  case "${1:-0}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

apt_install() {
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y \
    build-essential \
    ca-certificates \
    curl \
    git \
    jq \
    libssl-dev \
    libudev-dev \
    pkg-config \
    python3 \
    python3-pip \
    unzip
}

install_rust() {
  local requested="${RUST_TOOLCHAIN_VERSION:-}"
  if [[ -z "${requested}" && -f "${HALCYON_DIR}/rust-toolchain.toml" ]]; then
    requested="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "${HALCYON_DIR}/rust-toolchain.toml" | head -n1)"
  fi
  requested="${requested:-1.93.0}"

  if [[ ! -x /root/.cargo/bin/rustup ]]; then
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  fi
  export PATH="/root/.cargo/bin:${PATH}"
  rustup toolchain install "${requested}" --profile minimal
  rustup default "${requested}"
}

install_node() {
  local desired_major="${NODE_MAJOR:-20}"
  local current_major=""
  if command -v node >/dev/null 2>&1; then
    current_major="$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || true)"
  fi
  if [[ "${current_major}" != "${desired_major}" ]]; then
    curl -fsSL "https://deb.nodesource.com/setup_${desired_major}.x" | bash -
    apt-get install -y nodejs
  fi
  npm install -g tsx
}

install_solana() {
  local requested="${SOLANA_INSTALL_VERSION:-2.3.0}"
  local current=""
  if command -v solana >/dev/null 2>&1; then
    current="$(solana --version | awk '{print $2}')"
  fi
  if [[ "${current}" != "${requested}" ]]; then
    sh -c "$(curl -sSfL "https://release.anza.xyz/v${requested}/install")"
  fi
  local solana_bin_dir="/root/.local/share/solana/install/active_release/bin"
  install -d /usr/local/bin
  for bin in solana solana-keygen solana-test-validator; do
    if [[ -x "${solana_bin_dir}/${bin}" ]]; then
      ln -sf "${solana_bin_dir}/${bin}" "/usr/local/bin/${bin}"
    fi
  done
}

clone_or_update_repo() {
  require_env HALCYON_REPO_URL
  require_env HALCYON_BRANCH

  if [[ ! -d "${HALCYON_DIR}/.git" ]]; then
    git clone "${HALCYON_REPO_URL}" "${HALCYON_DIR}"
  fi

  git -C "${HALCYON_DIR}" fetch --all --prune
  if [[ -n "${HALCYON_REF:-}" ]]; then
    git -C "${HALCYON_DIR}" checkout --detach "${HALCYON_REF}"
  else
    git -C "${HALCYON_DIR}" checkout "${HALCYON_BRANCH}"
    git -C "${HALCYON_DIR}" pull --ff-only origin "${HALCYON_BRANCH}"
  fi
}

build_binaries() {
  export PATH="/root/.cargo/bin:/root/.local/share/solana/install/active_release/bin:${PATH}"
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
  cd "${HALCYON_DIR}"
  cargo build --release \
    -p observation_keeper \
    -p regime_keeper \
    -p regression_keeper \
    -p delta_keeper \
    -p hedge_keeper \
    -p flagship_hedge_keeper \
    -p halcyon_cli

  install -d "${BIN_DIR}"
  install -m 0755 \
    target/release/observation_keeper \
    target/release/regime_keeper \
    target/release/regression_keeper \
    target/release/delta_keeper \
    target/release/hedge_keeper \
    target/release/flagship_hedge_keeper \
    target/release/halcyon \
    "${BIN_DIR}/"

  cd "${HALCYON_DIR}/keepers/price_relay"
  npm ci --no-audit --no-fund

  cd "${HALCYON_DIR}/tools/mock_usdc_faucet"
  npm ci --no-audit --no-fund

  cd "${HALCYON_DIR}"
  local sigma_requirements="${HALCYON_DIR}/keepers/flagship_sigma_keeper/requirements.txt"
  if grep -Eq '^[[:space:]]*[^#[:space:]]' "${sigma_requirements}"; then
    python3 -m venv "${PYTHON_VENV_DIR}"
    "${PYTHON_VENV_DIR}/bin/python" -m pip install --upgrade pip
    "${PYTHON_VENV_DIR}/bin/python" -m pip install -r "${sigma_requirements}"
  fi
}

render_configs() {
  require_env HELIUS_DEVNET_RPC
  require_env HERMES_ENDPOINT
  require_env KERNEL_PROGRAM_ID
  require_env USDC_MINT
  require_env SOL_PROGRAM_ID
  require_env PYTH_SOL_ACCOUNT
  require_env PYTH_USDC_ACCOUNT
  require_env PYTH_SPY_ACCOUNT
  require_env PYTH_QQQ_ACCOUNT
  require_env PYTH_IWM_ACCOUNT
  require_env PRICE_RELAY_KEYPAIR
  require_env OBSERVATION_KEYPAIR
  require_env REGRESSION_KEYPAIR
  require_env DELTA_KEYPAIR
  require_env HEDGE_KEYPAIR
  require_env REGIME_KEYPAIR
  require_env FLAGSHIP_HEDGE_KEYPAIR

  local cache_feeds_json
  cache_feeds_json="$(printf '%s' "${PRICE_RELAY_CACHE_FEEDS:-SOL_USD,USDC_USD,SPY_USD,QQQ_USD,IWM_USD}" | jq -R 'split(",") | map(gsub("^\\s+|\\s+$"; "")) | map(select(length > 0))')"

  install -d "${CONFIG_DIR}" "${CALIBRATION_DIR}" /var/lib/halcyon /var/lib/halcyon/relay-cache /root/halcyon-keys

  cat > "${CONFIG_DIR}/price_relay.json" <<EOF
{
  "rpc_endpoint": "${HELIUS_DEVNET_RPC}",
  "hermes_endpoint": "${HERMES_ENDPOINT}",
  "keypair_path": "${PRICE_RELAY_KEYPAIR}",
  "shard_id": ${PRICE_RELAY_SHARD_ID:-7},
  "scan_interval_secs": ${PRICE_RELAY_SCAN_INTERVAL_SECS:-10},
  "staleness_cap_secs": ${PRICE_RELAY_STALENESS_CAP_SECS:-30},
  "cache_dir": "${PRICE_RELAY_CACHE_DIR:-/var/lib/halcyon/relay-cache}",
  "cache_retention_days": ${PRICE_RELAY_CACHE_RETENTION_DAYS:-450},
  "cache_feeds": ${cache_feeds_json},
  "failure_budget": ${PRICE_RELAY_FAILURE_BUDGET:-5},
  "backoff_cap_secs": ${PRICE_RELAY_BACKOFF_CAP_SECS:-60},
  "feeds": [
    { "alias": "SOL_USD",  "id": "0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d" },
    { "alias": "USDC_USD", "id": "0xeaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a" },
    { "alias": "SPY_USD",  "id": "0x19e09bb805456ada3979a7d1cbb4b6d63babc3a0f8e8a9509f68afa5c4c11cd5" },
    { "alias": "QQQ_USD",  "id": "0x9695e2b96ea7b3859da9ed25b7a46a920a776e2fdae19a7bcfdf2b219230452d" },
    { "alias": "IWM_USD",  "id": "0xeff690a187797aa225723345d4612abec0bf0cec1ae62347c0e7b1905d730879" }
  ]
}
EOF

  cat > "${CONFIG_DIR}/observation_keeper.json" <<EOF
{
  "rpc_endpoint": "${HELIUS_DEVNET_RPC}",
  "keypair_path": "${OBSERVATION_KEYPAIR}",
  "sol_autocall_program_id": "${SOL_PROGRAM_ID}",
  "usdc_mint": "${USDC_MINT}",
  "pyth_sol": "${PYTH_SOL_ACCOUNT}",
  "scan_interval_secs": ${OBSERVATION_SCAN_INTERVAL_SECS:-60},
  "backoff_cap_secs": ${OBSERVATION_BACKOFF_CAP_SECS:-60},
  "failure_budget": ${OBSERVATION_FAILURE_BUDGET:-5}
}
EOF

  cat > "${CONFIG_DIR}/regime_keeper.json" <<EOF
{
  "rpc_endpoint": "${HELIUS_DEVNET_RPC}",
  "keypair_path": "${REGIME_KEYPAIR}",
  "history_url": "${REGIME_HISTORY_URL:-https://api.coingecko.com/api/v3/coins/solana/market_chart?vs_currency=usd&days=120&interval=daily}",
  "scan_interval_secs": ${REGIME_SCAN_INTERVAL_SECS:-3600},
  "backoff_cap_secs": ${REGIME_BACKOFF_CAP_SECS:-300},
  "failure_budget": ${REGIME_FAILURE_BUDGET:-5}
}
EOF

  cat > "${CONFIG_DIR}/regression_keeper.json" <<EOF
{
  "rpc_endpoint": "${HELIUS_DEVNET_RPC}",
  "keypair_path": "${REGRESSION_KEYPAIR}",
  "pyth_spy": "${PYTH_SPY_ACCOUNT}",
  "pyth_qqq": "${PYTH_QQQ_ACCOUNT}",
  "pyth_iwm": "${PYTH_IWM_ACCOUNT}",
  "spy_history_url": "${REGRESSION_SPY_HISTORY_URL:-https://stooq.com/q/d/l/?s=spy.us&i=d}",
  "qqq_history_url": "${REGRESSION_QQQ_HISTORY_URL:-https://stooq.com/q/d/l/?s=qqq.us&i=d}",
  "iwm_history_url": "${REGRESSION_IWM_HISTORY_URL:-https://stooq.com/q/d/l/?s=iwm.us&i=d}",
  "scan_interval_secs": ${REGRESSION_SCAN_INTERVAL_SECS:-86400},
  "backoff_cap_secs": ${REGRESSION_BACKOFF_CAP_SECS:-900},
  "failure_budget": ${REGRESSION_FAILURE_BUDGET:-5}
}
EOF

  cat > "${CONFIG_DIR}/delta_keeper.json" <<EOF
{
  "rpc_endpoint": "${HELIUS_DEVNET_RPC}",
  "keypair_path": "${DELTA_KEYPAIR}",
  "pyth_spy": "${PYTH_SPY_ACCOUNT}",
  "pyth_qqq": "${PYTH_QQQ_ACCOUNT}",
  "pyth_iwm": "${PYTH_IWM_ACCOUNT}",
  "merkle_output_path": "${DELTA_MERKLE_OUTPUT_PATH:-/var/lib/halcyon/flagship_delta.json}",
  "scan_interval_secs": ${DELTA_SCAN_INTERVAL_SECS:-30},
  "backoff_cap_secs": ${DELTA_BACKOFF_CAP_SECS:-60},
  "failure_budget": ${DELTA_FAILURE_BUDGET:-5},
  "pinata_base_url": "${PINATA_BASE_URL:-https://api.pinata.cloud}",
  "pinata_retries": ${PINATA_RETRIES:-3}
}
EOF

  cat > "${CONFIG_DIR}/hedge_keeper.json" <<EOF
{
  "rpc_endpoint": "${HELIUS_DEVNET_RPC}",
  "keypair_path": "${HEDGE_KEYPAIR}",
  "kernel_program_id": "${KERNEL_PROGRAM_ID}",
  "sol_autocall_program_id": "${SOL_PROGRAM_ID}",
  "usdc_mint": "${USDC_MINT}",
  "pyth_sol": "${PYTH_SOL_ACCOUNT}",
  "jupiter_base_url": "https://api.jup.ag/swap/v1",
  "dry_run": ${HEDGE_DRY_RUN:-true},
  "allowed_jupiter_program_ids": [
    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"
  ],
  "allow_intraperiod_checks": true,
  "scan_interval_secs": ${HEDGE_SCAN_INTERVAL_SECS:-60},
  "backoff_cap_secs": ${HEDGE_BACKOFF_CAP_SECS:-60},
  "failure_budget": ${HEDGE_FAILURE_BUDGET:-5}
}
EOF

  cat > "${CONFIG_DIR}/flagship_hedge_keeper.json" <<EOF
{
  "rpc_endpoint": "${HELIUS_DEVNET_RPC}",
  "keypair_path": "${FLAGSHIP_HEDGE_KEYPAIR}",
  "pyth_spy": "${PYTH_SPY_ACCOUNT}",
  "pyth_qqq": "${PYTH_QQQ_ACCOUNT}",
  "pyth_iwm": "${PYTH_IWM_ACCOUNT}",
  "scan_interval_secs": ${FLAGSHIP_HEDGE_SCAN_INTERVAL_SECS:-300},
  "backoff_cap_secs": ${FLAGSHIP_HEDGE_BACKOFF_CAP_SECS:-60},
  "failure_budget": ${FLAGSHIP_HEDGE_FAILURE_BUDGET:-5},
  "aggregate_delta_max_age_secs": ${FLAGSHIP_HEDGE_AGGREGATE_DELTA_MAX_AGE_SECS:-1800},
  "aggregate_delta_max_spot_drift_bps": ${FLAGSHIP_HEDGE_AGGREGATE_DELTA_MAX_SPOT_DRIFT_BPS:-50},
  "regression_max_age_secs": ${FLAGSHIP_HEDGE_REGRESSION_MAX_AGE_SECS:-432000},
  "rebalance_cooldown_secs": ${FLAGSHIP_HEDGE_REBALANCE_COOLDOWN_SECS:-432000},
  "rebalance_breach_multiple_bps": ${FLAGSHIP_HEDGE_REBALANCE_BREACH_MULTIPLE_BPS:-15000},
  "leg_band_width_bps": ${FLAGSHIP_HEDGE_LEG_BAND_WIDTH_BPS:-1000},
  "leg_min_trade_bps": ${FLAGSHIP_HEDGE_LEG_MIN_TRADE_BPS:-100}
}
EOF
}

install_units() {
  install -m 0644 "${SCRIPT_DIR}"/systemd/* /etc/systemd/system/
  systemctl daemon-reload
}

enable_or_disable() {
  local enabled="$1"
  local unit="$2"
  if is_enabled "${enabled}"; then
    systemctl enable --now "${unit}"
  else
    systemctl disable --now "${unit}" >/dev/null 2>&1 || true
  fi
}

mask_or_unmask_service() {
  local enabled="$1"
  local unit="$2"
  if is_enabled "${enabled}"; then
    systemctl unmask "${unit}" >/dev/null 2>&1 || true
    systemctl enable --now "${unit}"
  else
    systemctl disable --now "${unit}" >/dev/null 2>&1 || true
    systemctl mask "${unit}" >/dev/null 2>&1 || true
  fi
}

main() {
  require_root
  load_env
  if is_enabled "${ENABLE_DELTA_KEEPER:-0}"; then
    require_env PINATA_JWT
  fi
  if is_enabled "${ENABLE_MOCK_USDC_FAUCET:-1}"; then
    require_env MOCK_USDC_MINT
    require_env MOCK_USDC_FAUCET_KEYPAIR_PATH
  fi
  apt_install
  clone_or_update_repo
  install_rust
  install_node
  install_solana
  build_binaries
  render_configs
  install_units

  enable_or_disable "${ENABLE_PRICE_RELAY:-1}" halcyon-price-relay.service
  enable_or_disable "${ENABLE_OBSERVATION_KEEPER:-1}" halcyon-observation-keeper.service
  enable_or_disable "${ENABLE_REGIME_KEEPER:-1}" halcyon-regime-keeper.service

  enable_or_disable "${ENABLE_IL_EWMA_TIMER:-1}" halcyon-update-ewma-il.timer
  enable_or_disable "${ENABLE_SOL_EWMA_TIMER:-1}" halcyon-update-ewma-sol.timer
  enable_or_disable "${ENABLE_FLAGSHIP_EWMA_TIMER:-0}" halcyon-update-ewma-flagship.timer
  enable_or_disable "${ENABLE_FLAGSHIP_SIGMA_TIMER:-1}" halcyon-flagship-sigma-keeper.timer
  enable_or_disable "${ENABLE_REDUCED_OPS_TIMER:-1}" halcyon-fire-reduced-ops.timer
  enable_or_disable "${ENABLE_AUTOCALL_SCHEDULE_TIMER:-1}" halcyon-write-autocall-schedule.timer
  enable_or_disable "${ENABLE_WRITE_REGRESSION_TIMER:-0}" halcyon-write-regression.timer

  enable_or_disable "${ENABLE_DELTA_KEEPER:-0}" halcyon-delta-keeper.service
  enable_or_disable "${ENABLE_HEDGE_KEEPER:-0}" halcyon-hedge-keeper.service
  enable_or_disable "${ENABLE_LEGACY_REGRESSION_KEEPER:-0}" halcyon-regression-keeper.service
  mask_or_unmask_service "${ENABLE_FLAGSHIP_HEDGE_KEEPER:-0}" halcyon-flagship-hedge-keeper.service
  enable_or_disable "${ENABLE_MOCK_USDC_FAUCET:-1}" halcyon-mock-usdc-faucet.service

  echo "Halcyon Hetzner deploy complete."
  echo "Repo: ${HALCYON_DIR}"
  echo "Configs: ${CONFIG_DIR}"
  echo "Env: ${ENV_FILE}"
  echo "Enabled core path: relay, observation, regime, IL/SOL EWMA timers, flagship sigma timer, reduced-ops timer, autocall-schedule timer"
}

main "$@"
