#!/usr/bin/env bash
# T01 baseline: per-crate `cargo test --no-run` smoke, isolated target dir, jobs=1
cd /d/Git/neo-rs || exit 1
export CARGO_TARGET_DIR=target-audit-v017-t01
export CARGO_INCREMENTAL=0
OUT=.t01-scratch
mkdir -p "$OUT"

run_one() {
  local name="$1"; shift
  local log="$OUT/${name}.log"
  local s=$(date +%s)
  cargo test -p "$name" "$@" --no-run --jobs 1 --offline > "$log" 2>&1
  local rc=$?
  local e=$(date +%s)
  echo "=== $name | ARGS: $* | EXIT=$rc | SECS=$((e-s)) ==="
  echo "$rc" > "$OUT/${name}.exit"
}

run_one neo-primitives
run_one neo-config
run_one neo-crypto
run_one neo-io
run_one neo-json
run_one neo-vm
run_one neo-storage
run_one neo-p2p
run_one neo-tee
run_one neo-hsm
run_one neo-telemetry
run_one neo-consensus
run_one neo-rpc --features server
run_one neo-core --features runtime
run_one neo-tests

echo "=== ALL DONE ==="
