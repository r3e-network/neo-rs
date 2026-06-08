#!/bin/bash
# Memory profiling script using heaptrack

set -e

PROFILE_DIR="target/profiling"
mkdir -p "$PROFILE_DIR"

# Check if heaptrack is installed
if ! command -v heaptrack &> /dev/null; then
    echo "heaptrack not found. Install with:"
    echo "  Ubuntu/Debian: sudo apt install heaptrack"
    echo "  Arch: sudo pacman -S heaptrack"
    exit 1
fi

# Build with release + debug symbols
echo "Building with profiling symbols..."
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release

# Run with heaptrack
echo "Running heaptrack..."
heaptrack -o "$PROFILE_DIR/heaptrack" target/release/neo-node "$@"

echo "Memory profile saved to $PROFILE_DIR/heaptrack.*.gz"
echo "Analyze with: heaptrack_gui $PROFILE_DIR/heaptrack.*.gz"
