# Performance Baselines

## Established: 2026-03-23

### Block Processing

- Deserialization: TBD (run `cargo bench` to establish)
- Validation: TBD

### State Root Calculation

- 1000 entries: TBD

### VM Execution

- Simple ADD operation: TBD

## How to Update Baselines

1. Run benchmarks:

    ```bash
    cargo bench
    ```

2. Record results in this file

3. CI will alert on >10% regression

## Target Improvements

- Block processing: 30-50% faster
- Memory usage: 20-30% reduction
- Latency: <100ms per block
