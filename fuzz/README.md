# Neo-rs Fuzzing Infrastructure

This directory contains the fuzzing infrastructure for neo-rs, using [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) with libFuzzer.

## Overview

Fuzzing is a security testing technique that provides random inputs to code to find crash/panic conditions. This is critical for blockchain software where malformed inputs could cause:

- Denial of service (DoS) attacks
- Node crashes
- Resource exhaustion
- Memory safety issues

## Fuzz Targets

### `fuzz_transaction_parse`

Tests transaction deserialization from raw bytes.

**Target:** `neo_core::network::p2p::payloads::transaction::Transaction`

**Vulnerabilities it finds:**
- Invalid transaction structure parsing
- Integer overflow in fee calculations
- Malformed signer/witness data
- Resource exhaustion via oversized fields

**Run:**
```bash
cd fuzz
cargo fuzz run fuzz_transaction_parse
```

### `fuzz_script_parse`

Tests VM script parsing and validation.

**Target:** `neo_vm::Script`

**Vulnerabilities it finds:**
- Invalid opcode sequences
- Jump targets outside script bounds
- Invalid instruction boundaries
- Stack overflow in script parsing

**Run:**
```bash
cd fuzz
cargo fuzz run fuzz_script_parse
```

### `fuzz_message_parse`

Tests P2P message deserialization.

**Target:** `neo_core::network::p2p::message::Message`

**Vulnerabilities it finds:**
- Malformed message headers
- Invalid compression (LZ4)
- Payload size attacks causing OOM
- Invalid command types

**Run:**
```bash
cd fuzz
cargo fuzz run fuzz_message_parse
```

## Prerequisites

1. Install Rust nightly:
```bash
rustup install nightly
```

2. Install cargo-fuzz:
```bash
cargo install cargo-fuzz
```

3. Install LLVM/Clang (for libFuzzer):
```bash
# Ubuntu/Debian
sudo apt-get install clang llvm-dev libclang-dev

# macOS
brew install llvm
```

## Running Fuzzers

### Run all fuzzers sequentially (quick test):
```bash
cd fuzz
cargo fuzz run fuzz_transaction_parse -- -max_total_time=60
cargo fuzz run fuzz_script_parse -- -max_total_time=60
cargo fuzz run fuzz_message_parse -- -max_total_time=60
```

### Run with specific options:
```bash
# Run with more workers (parallel fuzzing)
cargo fuzz run fuzz_transaction_parse -- -workers=4

# Run with specific seed
cargo fuzz run fuzz_transaction_parse -- -seed=12345

# Run until crash is found
cargo fuzz run fuzz_transaction_parse

# Run with max memory limit
cargo fuzz run fuzz_transaction_parse -- -max_len=4096 -rss_limit_mb=4096
```

### Release mode (recommended):
```bash
cargo fuzz run fuzz_transaction_parse --release
```

## Analyzing Crashes

When a crash is found, cargo-fuzz saves the crashing input to:
```
fuzz/artifacts/<target>/<crash-hash>
```

To reproduce the crash:
```bash
# The crash file can be fed directly to the fuzzer
cargo fuzz run fuzz_transaction_parse fuzz/artifacts/fuzz_transaction_parse/<crash-file>
```

To debug:
```bash
# Build the fuzzer in debug mode
cargo fuzz build fuzz_transaction_parse

# Run with the crash file and debugger
gdb ./target/debug/fuzz_transaction_parse
(gdb) run fuzz/artifacts/fuzz_transaction_parse/<crash-file>
```

## Minimizing Crashes

To create a minimal reproducer:
```bash
cargo fuzz tmin fuzz_transaction_parse fuzz/artifacts/fuzz_transaction_parse/<crash-file>
```

## Corpus

The fuzzer maintains a corpus of interesting inputs in:
```
fuzz/corpus/<target>/
```

These inputs are automatically discovered during fuzzing and help guide the fuzzer toward new code paths.

## CI Integration

Fuzzing runs automatically in CI:

- **PRs:** Quick 30-second smoke test per target
- **Nightly:** 5-minute full run per target
- **Manual:** Configurable duration via workflow dispatch

See `.github/workflows/fuzz.yml` for details.

## Security Considerations

When a crash is found:

1. **DO NOT** commit crash artifacts to the repository
2. **DO NOT** create public issues for security-critical crashes
3. Follow the project's security policy in `SECURITY.md`
4. Report privately to the maintainers

## Adding New Fuzz Targets

1. Create a new file in `fuzz/fuzz_targets/<target_name>.rs`
2. Add the binary to `fuzz/Cargo.toml`:
```toml
[[bin]]
name = "<target_name>"
path = "fuzz_targets/<target_name>.rs"
test = false
doc = false
```
3. Implement the fuzz target using the `fuzz_target!` macro
4. Add a job to `.github/workflows/fuzz.yml`

## Resources

- [cargo-fuzz documentation](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- [libFuzzer documentation](https://llvm.org/docs/LibFuzzer.html)
- [Rust Fuzz Book](https://rust-fuzz.github.io/book/)
