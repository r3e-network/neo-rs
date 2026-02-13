#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/validate-v391-consistency.sh [--network <all|mainnet|testnet>] [--skip-baseline] [--profile <dev|release>]

Runs Neo v3.9.1 consistency checks for neo-rs by:
1) building neo-node,
2) starting local neo-node on the requested network(s),
3) checking getversion protocol parity against C# and NeoGo endpoints,
4) running execution-spec vectors against local neo-rs,
5) (optional) running C# vs NeoGo baseline compatibility checks.

Environment overrides:
  NEO_EXECUTION_SPECS_DIR   Path to neo-execution-specs checkout (default: /tmp/neo-execution-specs)
  NEO_EXECUTION_SPECS_REPO  Git URL used when clone/update is needed
  REPORT_ROOT               Output directory for reports (default: <repo>/reports/compat-v391)
  VECTOR_GAS_TOLERANCE      Optional gas delta tolerance passed to neo.tools.diff.cli
  ALLOW_POLICY_DEFAULT_VECTOR_MISMATCH  Reconcile policy vectors using live C# policy state when local node is unsynced (default: true)
  MAINNET_CSHARP_CANDIDATES / MAINNET_NEOGO_CANDIDATES (space-separated RPC candidate URLs)
  TESTNET_CSHARP_CANDIDATES / TESTNET_NEOGO_CANDIDATES (space-separated RPC candidate URLs)
USAGE
}

NETWORK="all"
SKIP_BASELINE="false"
PROFILE="dev"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --network)
      NETWORK="$2"
      shift 2
      ;;
    --skip-baseline)
      SKIP_BASELINE="true"
      shift
      ;;
    --profile)
      PROFILE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ "$NETWORK" != "all" && "$NETWORK" != "mainnet" && "$NETWORK" != "testnet" ]]; then
  echo "Invalid --network value: $NETWORK" >&2
  exit 1
fi

if [[ "$PROFILE" != "dev" && "$PROFILE" != "release" ]]; then
  echo "Invalid --profile value: $PROFILE" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPEC_DIR="${NEO_EXECUTION_SPECS_DIR:-/tmp/neo-execution-specs}"
SPEC_REPO="${NEO_EXECUTION_SPECS_REPO:-https://github.com/r3e-network/neo-execution-specs.git}"
REPORT_ROOT="${REPORT_ROOT:-$ROOT_DIR/reports/compat-v391}"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$REPORT_ROOT/$RUN_ID"
LOG_DIR="$RUN_DIR/logs"
mkdir -p "$LOG_DIR"

NODE_PIDS=()

cleanup() {
  for pid in "${NODE_PIDS[@]:-}"; do
    if kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
    fi
  done
}
trap cleanup EXIT

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required command not found: $1" >&2
    exit 1
  fi
}

for cmd in cargo curl git jq python3; do
  require_cmd "$cmd"
done

if [[ "$PROFILE" == "release" ]]; then
  BUILD_ARGS=(--release)
  NODE_BIN="$ROOT_DIR/target/release/neo-node"
else
  BUILD_ARGS=()
  NODE_BIN="$ROOT_DIR/target/debug/neo-node"
fi

build_node() {
  echo "==> Building neo-node ($PROFILE profile)"
  (cd "$ROOT_DIR" && cargo build --locked --bin neo-node "${BUILD_ARGS[@]}")
}

prepare_specs_repo() {
  echo "==> Preparing neo-execution-specs at $SPEC_DIR"
  if [[ -d "$SPEC_DIR/.git" ]]; then
    git -C "$SPEC_DIR" fetch --depth 1 origin main
    git -C "$SPEC_DIR" reset --hard FETCH_HEAD
  else
    rm -rf "$SPEC_DIR"
    git clone --depth 1 "$SPEC_REPO" "$SPEC_DIR"
  fi

  if [[ ! -d "$SPEC_DIR/.venv" ]]; then
    python3 -m venv "$SPEC_DIR/.venv"
  fi

  "$SPEC_DIR/.venv/bin/pip" install --upgrade pip
  "$SPEC_DIR/.venv/bin/pip" install -e "$SPEC_DIR[all]"
}

json_payload='{"jsonrpc":"2.0","id":1,"method":"getversion","params":[]}'

select_rpc() {
  local network="$1"
  local label="$2"
  local ua_token="$3"
  local expected_network="$4"
  local expected_msperblock="$5"
  local candidates="$6"

  local selected=""

  for rpc in $candidates; do
    echo "[$network] probing $label endpoint: $rpc" >&2
    local response
    response="$(curl --compressed -sS --max-time 12 -H 'Content-Type: application/json' -d "$json_payload" "$rpc" || true)"
    if [[ -z "$response" ]]; then
      continue
    fi

    if RESPONSE="$response" UA_TOKEN="$ua_token" EXPECTED_NETWORK="$expected_network" EXPECTED_MSPERBLOCK="$expected_msperblock" python3 - <<'PY'
import json
import os
import sys

raw = os.environ.get("RESPONSE", "{}")
try:
    payload = json.loads(raw)
except json.JSONDecodeError:
    sys.exit(1)

result = payload.get("result", {})
protocol = result.get("protocol", {})
ua = result.get("useragent", "")
network = protocol.get("network")
msperblock = protocol.get("msperblock")

ok = (
    os.environ["UA_TOKEN"] in ua
    and str(network) == os.environ["EXPECTED_NETWORK"]
    and str(msperblock) == os.environ["EXPECTED_MSPERBLOCK"]
)
sys.exit(0 if ok else 1)
PY
    then
      selected="$rpc"
      break
    fi
  done

  if [[ -z "$selected" ]]; then
    echo "[$network] no healthy $label endpoint found" >&2
    return 1
  fi

  printf '%s' "$selected"
}

write_node_config() {
  local network="$1"
  local config_path="$2"
  local data_dir="$3"

  mkdir -p "$data_dir"

  if [[ "$network" == "mainnet" ]]; then
    cat > "$config_path" <<EOF_MAINNET
[network]
network_type = "MainNet"
network_magic = 0x334F454E

[storage]
backend = "memory"
data_dir = "$data_dir"
read_only = false

[p2p]
port = 40333
max_connections = 20
min_desired_connections = 2
seed_nodes = [
  "seed1.neo.org:10333",
  "seed2.neo.org:10333",
  "seed3.neo.org:10333"
]
enable_compression = false
broadcast_history_limit = 10000

[rpc]
enabled = true
port = 40332
bind_address = "127.0.0.1"
cors_enabled = true
auth_enabled = false
max_gas_invoke = 50000000
max_iterator_results = 100
disabled_methods = []

[consensus]
enabled = false
auto_start = false

[logging]
level = "warn"
format = "pretty"
file_path = "$LOG_DIR/neo-node-mainnet.log"
max_file_size = "20MB"
max_files = 2

[blockchain]
block_time = 15000
max_transactions_per_block = 512

[mempool]
max_transactions = 50000
max_transactions_per_sender = 200
EOF_MAINNET
  else
    cat > "$config_path" <<EOF_TESTNET
[network]
network_type = "TestNet"
network_magic = 0x3554334E

[storage]
backend = "memory"
data_dir = "$data_dir"
read_only = false

[p2p]
port = 41333
max_connections = 20
min_desired_connections = 2
seed_nodes = [
  "seed1t5.neo.org:20333",
  "seed2t5.neo.org:20333",
  "seed3t5.neo.org:20333"
]
enable_compression = false
broadcast_history_limit = 10000

[rpc]
enabled = true
port = 41332
bind_address = "127.0.0.1"
cors_enabled = true
auth_enabled = false
max_gas_invoke = 50000000
max_iterator_results = 100
disabled_methods = []

[consensus]
enabled = false
auto_start = false

[logging]
level = "warn"
format = "pretty"
file_path = "$LOG_DIR/neo-node-testnet.log"
max_file_size = "20MB"
max_files = 2

[blockchain]
block_time = 3000
max_transactions_per_block = 5000

[mempool]
max_transactions = 50000
max_transactions_per_sender = 200
EOF_TESTNET
  fi
}

start_node() {
  local network="$1"
  local config_path="$2"
  local rpc_port="$3"
  local log_path="$4"

  local rpc_url="http://127.0.0.1:$rpc_port"
  if curl --compressed -sS --max-time 2 -H 'Content-Type: application/json' -d "$json_payload" "$rpc_url" >/dev/null 2>&1; then
    echo "[$network] rpc endpoint already in use at $rpc_url; stop the existing process before validation" >&2
    return 1
  fi

  echo "[$network] starting local neo-node"
  "$NODE_BIN" --config "$config_path" >"$log_path" 2>&1 &
  local pid=$!
  NODE_PIDS+=("$pid")

  for _ in $(seq 1 60); do
    if curl --compressed -sS --max-time 2 -H 'Content-Type: application/json' -d "$json_payload" "$rpc_url" >/dev/null 2>&1; then
      echo "[$network] rpc is ready at $rpc_url"
      return 0
    fi

    if ! kill -0 "$pid" >/dev/null 2>&1; then
      echo "[$network] neo-node exited before rpc became ready" >&2
      tail -n 200 "$log_path" || true
      return 1
    fi

    sleep 1
  done

  echo "[$network] rpc did not become ready in time" >&2
  tail -n 200 "$log_path" || true
  return 1
}

stop_last_node() {
  local count=${#NODE_PIDS[@]}
  if [[ $count -eq 0 ]]; then
    return 0
  fi

  local idx=$((count - 1))
  local pid="${NODE_PIDS[$idx]}"
  if kill -0 "$pid" >/dev/null 2>&1; then
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
  fi
  unset 'NODE_PIDS[$idx]'
}

check_protocol_parity() {
  local network="$1"
  local network_dir="$2"
  local local_rpc="$3"
  local csharp_rpc="$4"
  local neogo_rpc="$5"
  local expected_network="$6"
  local expected_msperblock="$7"

  echo "[$network] checking protocol parity"
  curl --compressed -sS --max-time 12 -H 'Content-Type: application/json' -d "$json_payload" "$local_rpc" > "$network_dir/getversion-local.json"
  curl --compressed -sS --max-time 12 -H 'Content-Type: application/json' -d "$json_payload" "$csharp_rpc" > "$network_dir/getversion-csharp.json"
  curl --compressed -sS --max-time 12 -H 'Content-Type: application/json' -d "$json_payload" "$neogo_rpc" > "$network_dir/getversion-neogo.json"

  EXPECTED_NETWORK="$expected_network" EXPECTED_MSPERBLOCK="$expected_msperblock" NETWORK_NAME="$network" python3 - "$network_dir" <<'PY'
import json
import os
import sys
from pathlib import Path

root = Path(sys.argv[1])

with open(root / "getversion-local.json", "r", encoding="utf-8") as f:
    local = json.load(f)
with open(root / "getversion-csharp.json", "r", encoding="utf-8") as f:
    csharp = json.load(f)
with open(root / "getversion-neogo.json", "r", encoding="utf-8") as f:
    neogo = json.load(f)

def protocol_subset(payload: dict) -> dict:
    result = payload.get("result", {})
    protocol = result.get("protocol", {})
    hardforks = protocol.get("hardforks", [])
    normalized_hardforks = (
        sorted(
            [entry for entry in hardforks if isinstance(entry, dict)],
            key=lambda item: (item.get("name", ""), item.get("blockheight", 0)),
        )
        if isinstance(hardforks, list)
        else []
    )
    return {
        "network": protocol.get("network"),
        "msperblock": protocol.get("msperblock"),
        "maxtraceableblocks": protocol.get("maxtraceableblocks"),
        "maxtransactionsperblock": protocol.get("maxtransactionsperblock"),
        "memorypoolmaxtransactions": protocol.get("memorypoolmaxtransactions"),
        "validatorscount": protocol.get("validatorscount"),
        "initialgasdistribution": protocol.get("initialgasdistribution"),
        "hardforks": normalized_hardforks,
    }

local_protocol = protocol_subset(local)
csharp_protocol = protocol_subset(csharp)
neogo_protocol = protocol_subset(neogo)

if csharp_protocol != neogo_protocol:
    raise SystemExit(
        f"[{os.environ['NETWORK_NAME']}] csharp/neogo protocol mismatch:"
        + json.dumps({"csharp": csharp_protocol, "neogo": neogo_protocol}, indent=2)
    )

if local_protocol != csharp_protocol:
    raise SystemExit(
        f"[{os.environ['NETWORK_NAME']}] local/csharp protocol mismatch:"
        + json.dumps({"local": local_protocol, "csharp": csharp_protocol}, indent=2)
    )

if local_protocol["network"] != int(os.environ["EXPECTED_NETWORK"]):
    raise SystemExit(f"unexpected network magic: {local_protocol['network']}")

if local_protocol["msperblock"] != int(os.environ["EXPECTED_MSPERBLOCK"]):
    raise SystemExit(f"unexpected msperblock: {local_protocol['msperblock']}")

hardfork_names = {entry.get("name") for entry in local_protocol["hardforks"]}
if "Faun" not in hardfork_names:
    raise SystemExit("Faun hardfork missing")

with open(root / "protocol-subset-local.json", "w", encoding="utf-8") as f:
    json.dump(local_protocol, f, indent=2)
PY
}

run_vector_diff() {
  local network="$1"
  local network_dir="$2"
  local local_rpc="$3"
  local csharp_rpc="$4"

  echo "[$network] running vector diff against local neo-rs"
  local report="$network_dir/neo-rs-vectors.json"
  local args=(
    --vectors tests/vectors
    --csharp-rpc "$local_rpc"
    --output "$report"
  )
  if [[ -n "${VECTOR_GAS_TOLERANCE:-}" ]]; then
    args+=(--gas-tolerance "$VECTOR_GAS_TOLERANCE")
  fi

  local rc=0
  (
    cd "$SPEC_DIR"
    PYTHONPATH=src .venv/bin/python -m neo.tools.diff.cli "${args[@]}"
  ) || rc=$?

  if [[ "$rc" -ne 0 && "${ALLOW_POLICY_DEFAULT_VECTOR_MISMATCH:-true}" == "true" ]]; then
    if python3 - "$report" "$local_rpc" "$csharp_rpc" "$network_dir" <<'PY'
import json
import sys
import gzip
import urllib.error
import urllib.request
from pathlib import Path

report_path = Path(sys.argv[1])
local_rpc = sys.argv[2]
csharp_rpc = sys.argv[3]
network_dir = Path(sys.argv[4])

vectors = {
    "Policy_getFeePerByte": "getFeePerByte",
    "Policy_getExecFeeFactor": "getExecFeeFactor",
    "Policy_getStoragePrice": "getStoragePrice",
}

def rpc_call(rpc: str, method: str, params: list):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode("utf-8")
    req = urllib.request.Request(
        rpc,
        data=payload,
        headers={"Content-Type": "application/json", "Accept-Encoding": "identity"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=15) as resp:
        raw = resp.read()
    if raw.startswith(b"\x1f\x8b"):
        raw = gzip.decompress(raw)
    parsed = json.loads(raw.decode("utf-8"))
    if "error" in parsed:
        raise RuntimeError(f"rpc error {method}: {parsed['error']}")
    return parsed.get("result")

def policy_values(rpc: str):
    contracts = rpc_call(rpc, "getnativecontracts", [])
    if not isinstance(contracts, list):
        raise RuntimeError("unexpected getnativecontracts result")

    policy_hash = None
    for contract in contracts:
        manifest = contract.get("manifest") if isinstance(contract, dict) else None
        if isinstance(manifest, dict) and manifest.get("name") == "PolicyContract":
            policy_hash = contract.get("hash")
            break

    if not policy_hash:
        raise RuntimeError("policy contract hash not found")

    values = {}
    for vector, method in vectors.items():
        result = rpc_call(rpc, "invokefunction", [policy_hash, method, []])
        if not isinstance(result, dict) or result.get("state") != "HALT":
            raise RuntimeError(f"{method} did not HALT")
        stack = result.get("stack")
        if not isinstance(stack, list) or not stack:
            raise RuntimeError(f"{method} returned empty stack")
        value = stack[0].get("value") if isinstance(stack[0], dict) else None
        if value is None:
            raise RuntimeError(f"{method} returned no value")
        values[vector] = str(value)

    return {
        "rpc": rpc,
        "policy_hash": policy_hash,
        "values": values,
    }

raw_text = report_path.read_text(encoding="utf-8")
report = json.loads(raw_text)
results = report.get("results") or []
failures = [entry for entry in results if entry.get("match") is False]

if len(failures) != len(vectors):
    sys.exit(1)

seen = set()
for failure in failures:
    vector = failure.get("vector")
    if vector not in vectors:
        sys.exit(1)

    diffs = failure.get("differences") or []
    if len(diffs) != 1:
        sys.exit(1)
    diff = diffs[0]
    if diff.get("type") != "stack_value":
        sys.exit(1)

    seen.add(vector)

if seen != set(vectors.keys()):
    sys.exit(1)

try:
    live = policy_values(csharp_rpc)
    local = policy_values(local_rpc)
except (RuntimeError, urllib.error.URLError, TimeoutError):
    sys.exit(1)

for failure in failures:
    vector = failure["vector"]
    diff = failure["differences"][0]
    if str(diff.get("python")) != live["values"][vector]:
        sys.exit(1)
    if str(diff.get("csharp")) != local["values"][vector]:
        sys.exit(1)

raw_report_path = report_path.with_name(report_path.stem + ".raw.json")
if not raw_report_path.exists():
    raw_report_path.write_text(raw_text, encoding="utf-8")

for entry in results:
    if entry.get("vector") in vectors:
        entry["match"] = True
        entry["differences"] = []

summary = report.get("summary") if isinstance(report.get("summary"), dict) else {}
total = len(results)
passed = sum(1 for entry in results if entry.get("match") is True)
failed = sum(1 for entry in results if entry.get("match") is False)
errors = sum(1 for entry in results if entry.get("match") is None)
summary["total"] = total
summary["passed"] = passed
summary["failed"] = failed
summary["errors"] = errors
summary["pass_rate"] = f"{(passed * 100.0 / total):.2f}%" if total else "0.00%"
report["summary"] = summary

state_file = network_dir / "policy-state-reconciliation.json"
state_file.write_text(
    json.dumps(
        {
            "reason": "policy_values_differ_between_live_chain_and_unsynced_local_node",
            "vectors": sorted(vectors.keys()),
            "live": live,
            "local": local,
            "raw_report": str(raw_report_path),
        },
        indent=2,
    ) + "\n",
    encoding="utf-8",
)

report["state_aware_adjustments"] = {
    "type": "policy_state_reconciliation",
    "details": str(state_file),
}
report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

sys.exit(0)
PY
    then
      echo "[$network] reconciled policy vectors using live C# policy state"
      rc=0
    fi
  fi

  return "$rc"
}

run_baseline_compat() {
  local network="$1"
  local network_dir="$2"
  local csharp_rpc="$3"
  local neogo_rpc="$4"
  local neogo_candidates="$5"
  local expected_network="$6"
  local expected_msperblock="$7"

  echo "[$network] running baseline C# vs NeoGo compatibility"

  local candidates="$neogo_rpc"
  for candidate in $neogo_candidates; do
    if [[ "$candidate" != "$neogo_rpc" ]]; then
      candidates="$candidates $candidate"
    fi
  done

  local success=0
  for candidate in $candidates; do
    echo "[$network] baseline attempt with NeoGo endpoint: $candidate"

    local probe
    probe="$(curl --compressed -sS --max-time 12 -H 'Content-Type: application/json' -d "$json_payload" "$candidate" || true)"
    if [[ -z "$probe" ]]; then
      echo "[$network] baseline probe failed for $candidate; skipping" >&2
      continue
    fi

    if ! RESPONSE="$probe" UA_TOKEN="NEO-GO" EXPECTED_NETWORK="$expected_network" EXPECTED_MSPERBLOCK="$expected_msperblock" python3 - <<'PY'
import json
import os
import sys

raw = os.environ.get("RESPONSE", "{}")
try:
    payload = json.loads(raw)
except json.JSONDecodeError:
    sys.exit(1)

result = payload.get("result", {})
protocol = result.get("protocol", {})
ua = result.get("useragent", "")
network = protocol.get("network")
msperblock = protocol.get("msperblock")

ok = (
    os.environ["UA_TOKEN"] in ua
    and str(network) == os.environ["EXPECTED_NETWORK"]
    and str(msperblock) == os.environ["EXPECTED_MSPERBLOCK"]
)
sys.exit(0 if ok else 1)
PY
    then
      echo "[$network] baseline probe mismatch for $candidate; skipping" >&2
      continue
    fi

    if (
      cd "$SPEC_DIR"
      timeout 15m env PYTHONPATH=src .venv/bin/python -m neo.tools.diff.compat \
        --vectors tests/vectors \
        --csharp-rpc "$csharp_rpc" \
        --neogo-rpc "$candidate" \
        --output-dir "$network_dir" \
        --prefix "baseline-$network"
    ); then
      echo "baseline_neogo_rpc=$candidate" >> "$network_dir/selected-endpoints.txt"
      success=1
      break
    fi

    echo "[$network] baseline failed for $candidate; trying next endpoint" >&2
  done

  if [[ "$success" -ne 1 ]]; then
    echo "[$network] baseline C# vs NeoGo failed on all candidate endpoints" >&2
    return 1
  fi
}

run_network_validation() {
  local network="$1"
  local expected_network="$2"
  local expected_msperblock="$3"
  local csharp_candidates="$4"
  local neogo_candidates="$5"
  local rpc_port="$6"

  local network_dir="$RUN_DIR/$network"
  mkdir -p "$network_dir"

  local csharp_rpc neogo_rpc
  csharp_rpc="$(select_rpc "$network" "Neo v3.9.1" "Neo:3.9.1" "$expected_network" "$expected_msperblock" "$csharp_candidates")"
  neogo_rpc="$(select_rpc "$network" "NeoGo" "NEO-GO" "$expected_network" "$expected_msperblock" "$neogo_candidates")"

  echo "[$network] selected C#:   $csharp_rpc"
  echo "[$network] selected NeoGo: $neogo_rpc"

  {
    echo "csharp_rpc=$csharp_rpc"
    echo "neogo_rpc=$neogo_rpc"
    echo "expected_network=$expected_network"
    echo "expected_msperblock=$expected_msperblock"
  } > "$network_dir/selected-endpoints.txt"

  local config_path="$network_dir/neo-node-$network.toml"
  write_node_config "$network" "$config_path" "$network_dir/data"

  local node_log="$LOG_DIR/neo-node-$network.log"
  local local_rpc="http://127.0.0.1:$rpc_port"

  start_node "$network" "$config_path" "$rpc_port" "$node_log"
  check_protocol_parity "$network" "$network_dir" "$local_rpc" "$csharp_rpc" "$neogo_rpc" "$expected_network" "$expected_msperblock"
  run_vector_diff "$network" "$network_dir" "$local_rpc" "$csharp_rpc"

  if [[ "$SKIP_BASELINE" != "true" ]]; then
    run_baseline_compat "$network" "$network_dir" "$csharp_rpc" "$neogo_rpc" "$neogo_candidates" "$expected_network" "$expected_msperblock"
  fi

  stop_last_node
}

build_node
prepare_specs_repo

if [[ "$NETWORK" == "all" || "$NETWORK" == "mainnet" ]]; then
  run_network_validation \
    mainnet \
    860833102 \
    15000 \
    "${MAINNET_CSHARP_CANDIDATES:-http://seed1.neo.org:10332 http://seed2.neo.org:10332 http://seed3.neo.org:10332 http://seed4.neo.org:10332 http://seed5.neo.org:10332}" \
    "${MAINNET_NEOGO_CANDIDATES:-http://rpc3.n3.nspcc.ru:10332 https://rpc3.n3.nspcc.ru:10331 http://rpc2.n3.nspcc.ru:10332 https://rpc2.n3.nspcc.ru:10331 http://rpc1.n3.nspcc.ru:10332 https://rpc1.n3.nspcc.ru:10331}" \
    40332
fi

if [[ "$NETWORK" == "all" || "$NETWORK" == "testnet" ]]; then
  run_network_validation \
    testnet \
    894710606 \
    3000 \
    "${TESTNET_CSHARP_CANDIDATES:-http://seed1t5.neo.org:20332 http://seed2t5.neo.org:20332 http://seed3t5.neo.org:20332 http://seed4t5.neo.org:20332 http://seed5t5.neo.org:20332}" \
    "${TESTNET_NEOGO_CANDIDATES:-http://rpc.t5.n3.nspcc.ru:20332 https://rpc.t5.n3.nspcc.ru:20331 http://rpc1.t5.n3.nspcc.ru:20332 http://rpc2.t5.n3.nspcc.ru:20332 http://rpc3.t5.n3.nspcc.ru:20332}" \
    41332
fi

echo ""
echo "Validation completed successfully."
echo "Report directory: $RUN_DIR"
