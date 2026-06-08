#!/bin/bash
# Run benchmarks with flamegraph profiling

set -e

BENCH_DIR="target/criterion"

echo "Running benchmarks with profiling..."
cargo bench --bench block_processing -- --profile-time=5

echo "Benchmark results in $BENCH_DIR"
echo "Flamegraphs in $BENCH_DIR/*/profile/flamegraph.svg"
