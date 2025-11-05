#!/usr/bin/env bash

set -euo pipefail

run_test() {
  echo "\n=== cargo test $* ==="
  cargo test "$@"
}

echo "Running test suites..."

run_test --manifest-path neo-base/Cargo.toml
run_test --manifest-path neo-base/Cargo.toml --features derive
run_test --manifest-path neo-crypto/Cargo.toml
run_test --manifest-path neo-store/Cargo.toml
run_test --manifest-path neo-p2p/Cargo.toml
run_test --manifest-path neo-proc-macros/Cargo.toml
run_test --manifest-path integration-demo/Cargo.toml

echo "\nAll tests completed successfully."
