#!/bin/bash
# CPU profiling script using perf and flamegraph

set -e

PROFILE_DIR="target/profiling"
mkdir -p "$PROFILE_DIR"

# Build with release + debug symbols
echo "Building with profiling symbols..."
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release

# Run with perf
echo "Running perf record..."
perf record -F 99 -g --call-graph dwarf -o "$PROFILE_DIR/perf.data" \
    target/release/neo-node "$@"

# Generate flamegraph
echo "Generating flamegraph..."
perf script -i "$PROFILE_DIR/perf.data" | \
    stackcollapse-perf.pl | \
    flamegraph.pl > "$PROFILE_DIR/flamegraph.svg"

echo "Flamegraph saved to $PROFILE_DIR/flamegraph.svg"
