# Windows Build Guide

## Prerequisites
- Rust 1.95+ (install via rustup)
- Git for Windows
- Visual Studio Build Tools 2022

## Known Limitations
- **pprof profiler**: Unix-only, disabled on Windows
- Use `cargo build` without `--features profiling-full`

## Build Commands
```bash
# Standard build
cargo build --release -p neo-node

# With RocksDB
cargo build --release -p neo-node --features full
```

## Troubleshooting
- If link errors: Install VS Build Tools with C++ workload
- If OpenSSL errors: Use rustls (default, no action needed)
