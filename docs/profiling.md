# Performance Profiling Guide

## Overview

This guide covers CPU and memory profiling for neo-rs using industry-standard tools.

## Prerequisites

### Linux

```bash
# Ubuntu/Debian
sudo apt install linux-tools-common linux-tools-generic heaptrack

# Arch
sudo pacman -S perf heaptrack
```

### Flamegraph Tools

```bash
git clone https://github.com/brendangregg/FlameGraph
export PATH=$PATH:$PWD/FlameGraph
```

## CPU Profiling

### Quick Start

```bash
./scripts/profiling/cpu-profile.sh
```

### Manual Profiling

```bash
# Build with debug symbols
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release

# Record with perf
perf record -F 99 -g --call-graph dwarf target/release/neo-node

# Generate flamegraph
perf script | stackcollapse-perf.pl | flamegraph.pl > flamegraph.svg
```

## Memory Profiling

### Quick Start

```bash
./scripts/profiling/memory-profile.sh
```

### Analysis

```bash
heaptrack_gui target/profiling/heaptrack.*.gz
```

## Benchmarking

### Run Benchmarks

```bash
./scripts/profiling/benchmark.sh
```

### View Results

```bash
open target/criterion/report/index.html
```

## Interpreting Results

### CPU Hotspots

- Wide bars = high CPU time
- Focus on application code, not stdlib
- Target >1% of total time

### Memory Issues

- Look for allocation spikes
- Check for memory leaks (monotonic growth)
- Identify temporary allocations in hot paths

## Next Steps

After profiling:

1. Identify top 3 hotspots
2. Create targeted optimization tasks
3. Benchmark before/after changes
4. Verify protocol compatibility maintained
