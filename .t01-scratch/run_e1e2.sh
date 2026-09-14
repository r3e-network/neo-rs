#!/usr/bin/env bash
cd /d/Git/neo-rs || exit 1
export CARGO_TARGET_DIR=target-audit-v017-t01
export CARGO_INCREMENTAL=0
OUT=.t01-scratch
mkdir -p "$OUT"

echo "=== E1: cargo check -p neo-node (start $(date +%T)) ==="
s=$(date +%s)
cargo check -p neo-node --jobs 1 --offline > "$OUT/E1-neo-node.log" 2>&1
rc=$?
e=$(date +%s)
echo "E1 EXIT=$rc SECS=$((e-s))"
echo "=== E2: cargo check --manifest-path fuzz/Cargo.toml (start $(date +%T)) ==="
s=$(date +%s)
cargo check --manifest-path fuzz/Cargo.toml --jobs 1 --offline > "$OUT/E2-fuzz.log" 2>&1
rc=$?
e=$(date +%s)
echo "E2 EXIT=$rc SECS=$((e-s))"
echo "=== E1/E2 DONE ==="
