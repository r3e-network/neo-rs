#!/usr/bin/env bash
set -euo pipefail

# Neo Rust node Docker entrypoint. Chooses the correct config file, ensures
# persistent directories exist, and forwards any additional CLI arguments.

NETWORK="${NEO_NETWORK:-testnet}"
CONFIG_OVERRIDE="${NEO_CONFIG:-}"
STORAGE="${NEO_STORAGE:-}"
BACKEND="${NEO_BACKEND:-}"
PLUGINS_DIR="${NEO_PLUGINS_DIR:-/data/Plugins}"
LISTEN_PORT="${NEO_LISTEN_PORT:-}"

detect_rpc_port() {
  # Extract the RPC port from the TOML config's [rpc] section if present.
  local cfg="$1"
  awk '
    /^\[rpc\]/{in_rpc=1; next}
    in_rpc && /^\[/{exit}
    in_rpc && /^[[:space:]]*port[[:space:]]*=/{
      match($0, /[0-9]+/)
      if (RSTART > 0) {
        print substr($0, RSTART, RLENGTH)
        exit
      }
    }
  ' "$cfg" 2>/dev/null | head -n1
}

shopt -s nocasematch
if [[ -z "${CONFIG_OVERRIDE}" ]]; then
  case "${NETWORK}" in
    mainnet|main)
      CONFIG="/etc/neo/neo_mainnet_node.toml"
      RPC_PORT_DEFAULT=10332
      ;;
    testnet|test)
      CONFIG="/etc/neo/neo_testnet_node.toml"
      RPC_PORT_DEFAULT=20332
      ;;
    *)
      CONFIG="/etc/neo/neo_production_node.toml"
      RPC_PORT_DEFAULT=20332
      ;;
  esac
else
  CONFIG="${CONFIG_OVERRIDE}"
  RPC_PORT_DEFAULT=20332
fi
shopt -u nocasematch

if [[ -z "${STORAGE}" ]]; then
  shopt -s nocasematch
  case "${NETWORK}" in
    mainnet|main)
      STORAGE="/data/mainnet"
      ;;
    testnet|test)
      STORAGE="/data/testnet"
      ;;
    *)
      STORAGE="/data/blockchain"
      ;;
  esac
  shopt -u nocasematch
fi

if [[ ! -f "${CONFIG}" ]]; then
  echo "Config file not found at ${CONFIG}; set NEO_CONFIG to a valid path." >&2
  exit 1
fi

mkdir -p "${STORAGE}" "${PLUGINS_DIR}" /data/Logs
export NEO_PLUGINS_DIR="${PLUGINS_DIR}"

if ! touch "${STORAGE}/.write_test" >/dev/null 2>&1; then
  echo "Storage path ${STORAGE} is not writable; check volume permissions for user $(whoami)." >&2
  exit 1
fi
rm -f "${STORAGE}/.write_test" || true

if ! touch "${PLUGINS_DIR}/.write_test" >/dev/null 2>&1; then
  echo "Plugins path ${PLUGINS_DIR} is not writable; check volume permissions for user $(whoami)." >&2
  exit 1
fi
rm -f "${PLUGINS_DIR}/.write_test" || true

RPC_PORT="${NEO_RPC_PORT:-}"
if [[ -z "${RPC_PORT}" ]]; then
  RPC_PORT="$(detect_rpc_port "${CONFIG}")"
fi
if [[ -z "${RPC_PORT}" ]]; then
  RPC_PORT="${RPC_PORT_DEFAULT}"
fi

export NEO_RPC_PORT="${RPC_PORT}"
echo "${RPC_PORT}" > /tmp/neo_rpc_port || true

ARGS=(--config "${CONFIG}" --storage "${STORAGE}")

if [[ -n "${BACKEND}" ]]; then
  ARGS+=(--backend "${BACKEND}")
fi
if [[ -n "${LISTEN_PORT}" ]]; then
  ARGS+=(--listen-port "${LISTEN_PORT}")
fi

echo "neo-node starting with config=${CONFIG}, storage=${STORAGE}, backend=${BACKEND:-<default>}, listen_port=${LISTEN_PORT:-<config>}, plugins_dir=${PLUGINS_DIR}, rpc_port=${RPC_PORT}"

exec neo-node "${ARGS[@]}" "$@"
