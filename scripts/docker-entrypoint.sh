#!/usr/bin/env bash
set -euo pipefail

# Neo Rust node Docker entrypoint. Chooses the correct config file, ensures
# persistent directories exist, and forwards any additional CLI arguments.

NETWORK="${NEO_NETWORK:-testnet}"
PROFILE="${NEO_PROFILE:-}"
CONFIG_OVERRIDE="${NEO_CONFIG:-}"
CONFIG_ROOT="${NEO_CONFIG_ROOT:-/etc/neo}"
STORAGE="${NEO_STORAGE:-}"
BACKEND="${NEO_BACKEND:-}"
PLUGINS_DIR="${NEO_PLUGINS_DIR:-/data/Plugins}"
LOGS_DIR="${NEO_LOGS_DIR:-/data/Logs}"
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

prepare_container_service_config() {
  local src="$1"
  local dest
  local rpc_bind="${NEO_RPC_BIND_ADDRESS:-0.0.0.0}"
  local metrics_bind="${NEO_METRICS_BIND_ADDRESS:-${rpc_bind}}"
  dest="$(mktemp "${TMPDIR:-/tmp}/neo-node-config.XXXXXX")"

  awk \
    -v rpc_bind="${rpc_bind}" \
    -v metrics_bind="${metrics_bind}" \
    -v storage_root="${STORAGE}" \
    -v logs_root="${LOGS_DIR}" '
    function rewrite_quoted_path(line, marker, target_root,    quote, prefix, rest, end_quote, raw_path, trailer, slash, suffix) {
      quote = index(line, "\"")
      if (quote == 0) {
        return line
      }
      prefix = substr(line, 1, quote)
      rest = substr(line, quote + 1)
      if (index(rest, marker) != 1) {
        return line
      }
      end_quote = index(rest, "\"")
      if (end_quote == 0) {
        return line
      }
      raw_path = substr(rest, length(marker) + 1, end_quote - length(marker) - 1)
      trailer = substr(rest, end_quote)
      if (marker == "./data/") {
        slash = index(raw_path, "/")
        suffix = slash == 0 ? "" : substr(raw_path, slash)
      } else {
        suffix = "/" raw_path
      }
      return prefix target_root suffix trailer
    }

    /^\[/ { section = $0 }
    section == "[rpc]" && /^[[:space:]]*bind_address[[:space:]]*=/ {
      print "bind_address = \"" rpc_bind "\""
      next
    }
    section == "[telemetry.metrics]" && /^[[:space:]]*bind_address[[:space:]]*=/ {
      print "bind_address = \"" metrics_bind "\""
      next
    }
    /=[[:space:]]*"\.\/data\// {
      print rewrite_quoted_path($0, "./data/", storage_root)
      next
    }
    /=[[:space:]]*"\.\/logs\// {
      print rewrite_quoted_path($0, "./logs/", logs_root)
      next
    }
    { print }
  ' "${src}" > "${dest}"

  echo "${dest}"
}

SERVICE_PROFILE_CONFIG=false
shopt -s nocasematch
case "${PROFILE}" in
  ""|default|node|service)
    ;;
  *)
    echo "unsupported NEO_PROFILE=${PROFILE}; expected empty, default, node, or service." >&2
    exit 1
    ;;
esac

if [[ -z "${CONFIG_OVERRIDE}" ]]; then
  if [[ "${PROFILE}" == "service" ]]; then
    case "${NETWORK}" in
      mainnet|main)
        CONFIG="${CONFIG_ROOT}/config/mainnet-service.toml"
        RPC_PORT_DEFAULT=10332
        SERVICE_PROFILE_CONFIG=true
        ;;
      testnet|test)
        CONFIG="${CONFIG_ROOT}/config/testnet-service.toml"
        RPC_PORT_DEFAULT=20332
        SERVICE_PROFILE_CONFIG=true
        ;;
      *)
        echo "NEO_PROFILE=service requires NEO_NETWORK=mainnet or testnet." >&2
        exit 1
        ;;
    esac
  else
    case "${NETWORK}" in
      mainnet|main)
        CONFIG="${CONFIG_ROOT}/neo_mainnet_node.toml"
        RPC_PORT_DEFAULT=10332
        ;;
      testnet|test)
        CONFIG="${CONFIG_ROOT}/neo_testnet_node.toml"
        RPC_PORT_DEFAULT=20332
        ;;
      *)
        CONFIG="${CONFIG_ROOT}/neo_production_node.toml"
        RPC_PORT_DEFAULT=20332
        ;;
    esac
  fi
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

if [[ "${SERVICE_PROFILE_CONFIG}" == "true" ]]; then
  CONFIG="$(prepare_container_service_config "${CONFIG}")"
fi

mkdir -p "${STORAGE}" "${PLUGINS_DIR}" "${LOGS_DIR}"
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

# The neo-node CLI accepts only --config/-c, --storage-path and --network-magic.
# Storage backend, listener port and RPC settings come from the TOML config,
# not CLI flags.
ARGS=(--config "${CONFIG}")

if [[ -n "${STORAGE}" ]]; then
  ARGS+=(--storage-path "${STORAGE}")
fi

echo "neo-node starting with config=${CONFIG}, storage_path=${STORAGE:-<config>}, plugins_dir=${PLUGINS_DIR}, rpc_port=${RPC_PORT}"

exec neo-node "${ARGS[@]}" "$@"
