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

## Toolchain and target-cpu

Measured 2026-10-29 on a 24-thread Windows workstation. `neo-benches` suites:
hashing, vm_execution, p2p_frames, payload_serialization, native_contracts,
mpt_state_root. Bench profile is `lto = "fat"`, `codegen-units = 1`.

Method: prebuilt bench binaries were run interleaved (A, B, C, … then reversed)
for three rounds, and the per-benchmark median was taken. The baseline is Rust
1.95.0 `x86_64-pc-windows-gnullvm`. A negative percent is faster.

| Configuration | Host | Geomean vs 1.95 | Median vs 1.95 |
|---|---|---|---|
| Rust 1.99.1 | gnullvm | −2.7% | −0.5% |
| Rust 1.99.1 + `-C target-cpu=x86-64-v3` | gnullvm | +0.8% | +1.6% |
| Rust 1.99.1 | msvc | −2.9% | −0.3% |
| Rust 1.99.1, PGO (`scripts/pgo-build.sh`) | msvc | −4.5% | −1.4% |

Same-host changes from 1.95 to 1.99.1 on gnullvm, largest first:

| Benchmark | 1.95 | 1.99.1 |
|---|---|---|
| `stack_push_pop_1000` | 12.2 µs | −52% |
| `vm_nop_5000` | 3.72 µs | −12% |
| `p2p_message_deserialize_500tx_compressed` | 2.60 µs | −9% |
| `vm_add_loop_1000` | 97.5 µs | −8% |
| `sha256/32_bytes` | 34.9 ns | −7% |
| `p2p_block_encode_500tx_compressed` | 27.6 µs | +8% |
| `hash256/1024_bytes` | 262 ns | +5% |

Conclusions:

* Upgrading 1.95 to 1.99.1 on the same host is a small win overall. The VM
  stack path is much faster. The only clear slowdown in this set is compressed
  block encoding.
* `-C target-cpu=x86-64-v3` is slower in this set (geomean +0.8%) and the
  binary will not start on CPUs without AVX2/BMI2. It stays opt-in only.
* PGO on the MSVC host is the fastest of the four. About 1.6 points of the
  −4.5% come from the profile, and the profile was collected by running these
  same benches, so that extra is an in-sample upper bound. A mainnet replay
  training run is the next measurement (`PGO_TRAIN_CMD`).
* Instrumented binaries crash during training on `x86_64-pc-windows-gnullvm`
  (Rust 1.99.1, `hashing` suite, exit 139). The PGO script is for the MSVC host
  on Windows, and for Linux.

## Target Improvements

- Block processing: 30-50% faster
- Memory usage: 20-30% reduction
- Latency: <100ms per block
