#!/usr/bin/env bash
#
# pgo-build.sh — profile-guided optimisation build of neo-node.
# Policy and rationale: docs/TOOLCHAIN.md#profile-guided-optimisation.
#
# Pipeline:
#   1. instrument  Build with -Cprofile-generate.
#   2. train       Default: run the neo-benches criterion suites (VM, hashing,
#                  serialization, MPT, P2P framing). With PGO_TRAIN_CMD set, an
#                  instrumented neo-node is built instead and the command runs
#                  with $NEO_NODE pointing at it (e.g. an --import-acc replay).
#   3. merge       llvm-profdata merge -> merged.profdata.
#   4. optimise    Build neo-node (--profile production) with -Cprofile-use.
#
# Environment:
#   PGO_FEATURES         neo-node features for the final build (default: full)
#   PGO_TRAIN_CMD        custom training command; runs via `bash -c`
#   PGO_COMPARE_BASELINE criterion baseline name; when set, the benches are
#                        re-run with -Cprofile-use and compared against it
#
# Output: target/pgo/production/neo-node

set -euo pipefail
cd "$(dirname "$0")/.."

FEATURES="${PGO_FEATURES:-full}"
# rustc needs a native path; Git Bash on Windows reports /d/... from plain pwd.
ROOT="$(pwd -W 2>/dev/null || pwd)"
PGO_DIR="$ROOT/target/pgo"
PROFILES="$PGO_DIR/profiles"
BENCHES=(--bench payload_serialization --bench hashing --bench vm_execution
         --bench native_contracts --bench mpt_state_root --bench p2p_frames)
CRITERION_QUICK=(--warm-up-time 1 --measurement-time 2 --noplot)

rustup component add llvm-tools >/dev/null
HOST="$(rustc -vV | sed -n 's/^host: //p')"
PROFDATA="$(rustc --print sysroot)/lib/rustlib/$HOST/bin/llvm-profdata"
[[ -x "$PROFDATA" || -x "$PROFDATA.exe" ]] || { echo "llvm-profdata not found at $PROFDATA" >&2; exit 1; }

rm -rf "$PROFILES"
mkdir -p "$PROFILES"
GEN_FLAGS="-Cprofile-generate=$PROFILES"
USE_FLAGS="-Cprofile-use=$PROFILES/merged.profdata -Cllvm-args=-no-pgo-warn-mismatch"

echo "==> [1-2] instrumented training run"
if [[ -n "${PGO_TRAIN_CMD:-}" ]]; then
  RUSTFLAGS="$GEN_FLAGS" cargo build --locked --profile production -p neo-node \
    --features "$FEATURES" --target-dir "$PGO_DIR/instrumented"
  NEO_NODE="$PGO_DIR/instrumented/production/neo-node" bash -c "$PGO_TRAIN_CMD"
else
  RUSTFLAGS="$GEN_FLAGS" cargo bench --locked -p neo-benches "${BENCHES[@]}" \
    --target-dir "$PGO_DIR/instrumented" -- "${CRITERION_QUICK[@]}"
fi

echo "==> [3] merge profiles"
"$PROFDATA" merge -o "$PROFILES/merged.profdata" "$PROFILES"

echo "==> [4] optimised build"
RUSTFLAGS="$USE_FLAGS" cargo build --locked --profile production -p neo-node \
  --features "$FEATURES" --target-dir "$PGO_DIR"
echo "PGO binary: $PGO_DIR/production/neo-node"

if [[ -n "${PGO_COMPARE_BASELINE:-}" ]]; then
  echo "==> comparing PGO benches against criterion baseline '$PGO_COMPARE_BASELINE'"
  RUSTFLAGS="$USE_FLAGS" cargo bench --locked -p neo-benches "${BENCHES[@]}" \
    -- "${CRITERION_QUICK[@]}" --baseline "$PGO_COMPARE_BASELINE"
fi
