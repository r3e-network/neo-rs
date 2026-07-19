#!/usr/bin/env bash
set -uo pipefail
# Validate our stored state roots against C# mainnet reference
# Usage: ./validate_stored_roots.sh [start_height] [end_height] [step]

START=${1:-172600}
END=${2:-193500}
STEP=${3:-1000}

CSHARP_RPC="${CSHARP_RPC:-https://mainnet1.neo.coz.io:443}"
BIN="${LOCAL_ROOT_BIN:-target/release/examples/check_local_roots}"
CURL_BIN="${CURL_BIN:-curl}"

for value in "$START" "$END" "$STEP"; do
    if [[ ! "$value" =~ ^[0-9]+$ ]]; then
        echo "invalid range: start, end, and step must be decimal integers" >&2
        exit 2
    fi
done
if (( START < 0 || END < START || STEP <= 0 )); then
    echo "invalid range: require 0 <= start <= end and step > 0" >&2
    exit 2
fi
if [[ ! -x "$BIN" ]]; then
    echo "local root probe is not executable: $BIN" >&2
    exit 2
fi

echo "Validating stored state roots from $START to $END (step $STEP)"
echo "Local vs C# reference ($CSHARP_RPC)"
echo ""

matches=0
mismatches=0
missing=0
query_failures=0
first_mismatch=""

for ((h=START; h<=END; h+=STEP)); do
    local_output=""
    if ! local_output=$("$BIN" "$h" 2>&1); then
        echo "  $h: (local query failed)"
        query_failures=$((query_failures+1))
        continue
    fi
    local_line=$(printf '%s\n' "$local_output" | awk -v prefix="height=$h" '
        index($0, prefix) == 1 &&
        (length($0) == length(prefix) || substr($0, length(prefix) + 1, 1) == " ") {
            print
            exit
        }
    ')
    if [[ -z "$local_line" ]]; then
        echo "  $h: (local query failed)"
        query_failures=$((query_failures+1))
        continue
    fi
    if [[ "$local_line" == *"no local root"* ]]; then
        echo "  $h: (no local data)"
        missing=$((missing+1))
        continue
    fi
    if [[ "$local_line" =~ root=(0x[0-9a-fA-F]{64})([[:space:]]|$) ]]; then
        local_r="${BASH_REMATCH[1],,}"
    else
        echo "  $h: (local root response malformed)"
        query_failures=$((query_failures+1))
        continue
    fi
    csharp_response=""
    if ! csharp_response=$("$CURL_BIN" --fail --silent --show-error --max-time 20 -X POST "$CSHARP_RPC" -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getstateroot\",\"params\":[$h]}" ); then
        echo "  $h: (C# query failed)"
        query_failures=$((query_failures+1))
        continue
    fi
    csharp_r=""
    if ! csharp_r=$(printf '%s' "$csharp_response" | python3 -c "import json,re,sys;d=json.load(sys.stdin);r=d.get('result',{}).get('roothash');assert re.fullmatch(r'0x[0-9a-fA-F]{64}',r);print(r.lower())" 2>/dev/null); then
        echo "  $h: (C# response malformed)"
        query_failures=$((query_failures+1))
        continue
    fi
    if [[ -z "$csharp_r" ]]; then
        echo "  $h: (C# query failed)"
        query_failures=$((query_failures+1))
        continue
    fi
    if [[ "$local_r" == "$csharp_r" ]]; then
        matches=$((matches+1))
    else
        if [[ -z "$first_mismatch" ]]; then
            first_mismatch=$h
        fi
        mismatches=$((mismatches+1))
        echo "  $h MISMATCH local=${local_r:0:18}... csharp=${csharp_r:0:18}..."
    fi
done

echo ""
echo "=== Summary ==="
echo "  matches:    $matches"
echo "  mismatches: $mismatches"
echo "  missing:    $missing"
echo "  query failures: $query_failures"
if [[ -n "$first_mismatch" ]]; then
    echo "  first mismatch: block $first_mismatch"
fi

expected=$(((END - START) / STEP + 1))
if (( mismatches > 0 || missing > 0 || query_failures > 0 || matches != expected )); then
    echo "FAIL: requested range was not completely verified" >&2
    exit 1
fi
echo "PASS: all $expected requested roots matched"
