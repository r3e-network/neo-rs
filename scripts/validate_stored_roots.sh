#!/bin/bash
# Validate our stored state roots against C# mainnet reference
# Usage: ./validate_stored_roots.sh [start_height] [end_height] [step]

START=${1:-172600}
END=${2:-193500}
STEP=${3:-1000}

CSHARP_RPC="https://mainnet1.neo.coz.io:443"
BIN="target/release/examples/check_local_roots"

echo "Validating stored state roots from $START to $END (step $STEP)"
echo "Local vs C# reference ($CSHARP_RPC)"
echo ""

matches=0
mismatches=0
missing=0
first_mismatch=""

for ((h=START; h<=END; h+=STEP)); do
    local_line=$($BIN $h 2>&1 | grep "^height=$h")
    if [[ "$local_line" == *"no local root"* ]]; then
        echo "  $h: (no local data)"
        missing=$((missing+1))
        continue
    fi
    local_r=$(echo "$local_line" | sed -E 's/.*root=(0x[0-9a-f]+).*/\1/')
    csharp_r=$(curl -s -X POST "$CSHARP_RPC" -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getstateroot\",\"params\":[$h]}" \
        | python3 -c "import json,sys;d=json.load(sys.stdin);print(d['result']['roothash'])" 2>/dev/null)
    if [[ -z "$csharp_r" ]]; then
        echo "  $h: (C# query failed)"
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
if [[ -n "$first_mismatch" ]]; then
    echo "  first mismatch: block $first_mismatch"
fi
