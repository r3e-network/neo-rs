# Benchmark Regression Testing

This document describes the benchmark regression testing infrastructure for neo-rs.

## Overview

The benchmark regression system tracks performance over time and automatically detects when code changes introduce performance regressions. It uses Criterion.rs for benchmarking and compares results against a baseline.

## Components

### 1. GitHub Actions Workflow (`.github/workflows/benchmark-regression.yml`)

Automated CI pipeline that:
- Runs on pull requests and pushes to main/master
- Compares PR benchmarks against main branch baseline
- Posts results as PR comments
- Fails CI if regressions exceed threshold (default: 10%)
- Updates baseline on main branch merges

### 2. Benchmark Runner Script (`scripts/bench-regression.sh`)

Bash script that orchestrates benchmark execution:

```bash
# Save initial baseline
./scripts/bench-regression.sh --save-baseline

# Compare against baseline
./scripts/bench-regression.sh --compare --threshold 10
```

**Options:**
- `--save-baseline`: Run benchmarks and save as baseline
- `--compare`: Run benchmarks and compare to baseline
- `--threshold <N>`: Set regression threshold percentage (default: 10)
- `--verbose`: Show detailed output

### 3. Analysis Script (`scripts/analyze-bench.py`)

Python script that analyzes Criterion results:
- Parses Criterion JSON output
- Calculates performance changes
- Categorizes as regression/improvement/no-change
- Generates JSON report
- Returns exit code 1 if regressions found

## Usage

### Local Development

**First time setup:**
```bash
# Generate baseline from current main branch
git checkout main
./scripts/bench-regression.sh --save-baseline
```

**Before submitting PR:**
```bash
# Check for regressions
git checkout your-feature-branch
./scripts/bench-regression.sh --compare

# Review results in benchmark-results/regression-report.json
```

**Run specific benchmarks:**
```bash
# Run only VM benchmarks
cargo bench -p neo-benches vm_execution

# Run only crypto benchmarks
cargo bench -p neo-crypto
```

### CI/CD Integration

The workflow automatically:
1. **On PR**: Compares PR benchmarks vs main branch baseline
2. **On main merge**: Updates baseline for future comparisons
3. **Reports**: Posts detailed results as PR comments
4. **Artifacts**: Uploads full results (30-day retention)

### Interpreting Results

**Regression Report Format:**
```json
{
  "threshold_percent": 10,
  "total_benchmarks": 25,
  "regressions": [
    {
      "name": "vm_execution/vm_add_loop_1000",
      "change_percent": 15.5,
      "confidence": 0.95
    }
  ],
  "improvements": [
    {
      "name": "serialization/transaction_hash",
      "change_percent": -8.2,
      "confidence": 0.95
    }
  ],
  "no_change": [...]
}
```

**Change Types:**
- **Regression**: Performance decreased by > threshold (slower)
- **Improvement**: Performance increased (faster)
- **No Change**: Change within threshold

### Threshold Guidelines

- **10%** (default): Standard for most changes
- **5%**: Strict mode for performance-critical code
- **15-20%**: Acceptable for major refactors

Set via environment variable:
```bash
export REGRESSION_THRESHOLD=5
./scripts/bench-regression.sh --compare
```

## Existing Benchmarks

### VM Execution (`benches-package/benches/vm_execution.rs`)
- Opcode dispatch overhead
- Stack operations
- Script parsing and validation
- Script building

### Serialization (`neo-core/benches/serialization.rs`)
- Transaction serialization/deserialization
- Header serialization/deserialization
- Binary writer operations
- Hash computation

### Crypto Operations (`neo-crypto/benches/crypto_ops.rs`)
- Cryptographic primitives
- Hash functions
- Signature verification

### Storage (`neo-core/benches/storage.rs`)
- Database operations
- State store operations

### State Service (`neo-core/benches/`)
- Account prefetch (`account_prefetch_bench.rs`)
- Prefetch pipeline (`prefetch_pipeline.rs`)

## Adding New Benchmarks

1. **Create benchmark file:**
```rust
// neo-module/benches/my_benchmark.rs
use criterion::{criterion_group, criterion_main, Criterion, black_box};

fn bench_my_feature(c: &mut Criterion) {
    c.bench_function("my_feature", |b| {
        b.iter(|| {
            // Benchmark code
            black_box(my_function());
        });
    });
}

criterion_group!(benches, bench_my_feature);
criterion_main!(benches);
```

2. **Register in Cargo.toml:**
```toml
[[bench]]
name = "my_benchmark"
harness = false
```

3. **Run and verify:**
```bash
cargo bench -p neo-module my_benchmark
```

## Best Practices

1. **Baseline Management:**
   - Update baseline after merging performance improvements
   - Re-baseline after major refactors
   - Keep baselines in sync with main branch

2. **Benchmark Design:**
   - Use `black_box()` to prevent optimizer eliminating code
   - Warm up before measuring (Criterion handles this)
   - Test realistic workloads, not micro-optimizations
   - Include both hot and cold path scenarios

3. **Handling Regressions:**
   - Investigate root cause before merging
   - Document intentional tradeoffs
   - Consider adjusting threshold for specific cases
   - Add comments explaining performance characteristics

4. **CI Configuration:**
   - Run on dedicated hardware when possible
   - Account for CI runner variance
   - Use consistent runner types
   - Consider time-of-day effects

## Troubleshooting

**No baseline found:**
```bash
./scripts/bench-regression.sh --save-baseline
```

**Python script not found:**
- Ensure Python 3 is installed
- Script falls back to basic analysis

**High variance in results:**
- Run multiple iterations: `cargo bench -- --sample-size 100`
- Check for background processes
- Use consistent hardware

**Cache issues:**
- Clear criterion cache: `rm -rf target/criterion`
- Regenerate baseline

## Future Enhancements

- [ ] Historical performance tracking database
- [ ] Performance dashboards (e.g., with Grafana)
- [ ] Flamegraph integration for profiling
- [ ] Automatic bisection for regression source
- [ ] Per-component thresholds
- [ ] Statistical significance testing
- [ ] Memory usage tracking

## References

- [Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Benchmarking Best Practices](https://easyperf.net/blog/)
