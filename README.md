# Neo-RS: Neo N3 Blockchain Implementation in Rust

[![Build Status](https://github.com/r3e-network/neo-rs/workflows/CI/badge.svg)](https://github.com/r3e-network/neo-rs/actions)
[![Crates.io](https://img.shields.io/crates/v/neo-rs.svg)](https://crates.io/crates/neo-rs)
[![Documentation](https://docs.rs/neo-rs/badge.svg)](https://docs.rs/neo-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A high-performance, production-ready implementation of the Neo N3 blockchain protocol written in Rust.

## Features

- **High Performance**: Optimized for throughput and low latency
- **Production Ready**: Comprehensive error handling and monitoring
- **Modular Design**: Composable components for different use cases
- **Full Compatibility**: Compatible with Neo N3 protocol specification
- **Type Safety**: Leverages Rust's type system for correctness

## Architecture

Neo-RS consists of several core crates:

- **neo-core** - Core blockchain types and utilities
- **neo-vm** - Neo Virtual Machine implementation
- **neo-consensus** - dBFT 2.0 consensus algorithm
- **neo-network** - P2P networking and protocol
- **neo-ledger** - Blockchain state and transaction processing
- **neo-persistence** - Storage and database abstractions
- **neo-cryptography** - Cryptographic primitives
- **neo-smart-contract** - Smart contract execution engine

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
neo-rs = "0.3.0"
```

### Basic Usage

```rust
use neo_rs::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize Neo node configuration
    let config = NodeConfig::default();
    
    // Start the Neo node
    let mut node = NeoNode::new(config).await?;
    node.start().await?;
    
    println!("Neo node started successfully!");
    
    // Your application logic here...
    
    node.stop().await?;
    Ok(())
}
```

## Building

### Prerequisites

- Rust 1.70 or later
- System dependencies for RocksDB (see below)

### System Dependencies

**Ubuntu/Debian:**
```bash
sudo apt-get install build-essential clang librocksdb-dev
```

**macOS:**
```bash
brew install rocksdb
```

**Windows:**
```bash
# Install Visual Studio Build Tools
# RocksDB will be built from source
```

### Build Commands

```bash
# Clone the repository
git clone https://github.com/r3e-network/neo-rs.git
cd neo-rs

# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Build documentation
cargo doc --workspace --no-deps

# Run with all features
cargo run --features full
```

## Features

The following cargo features are available:

- `full` - Enable all features (default)
- `consensus` - dBFT 2.0 consensus support
- `rpc` - JSON-RPC server and client
- `metrics` - Prometheus metrics collection
- `compression` - Data compression support

### Feature Examples

```bash
# Minimal build
cargo build --no-default-features

# With consensus only
cargo build --no-default-features --features consensus

# With RPC only
cargo build --no-default-features --features rpc
```

## Documentation

- [API Documentation](https://docs.rs/neo-rs)
- [Neo Protocol Documentation](https://docs.neo.org/)

## Configuration

### Node Configuration

```rust
use neo_rs::{NodeConfig, NetworkConfig, StorageConfig};

let config = NodeConfig {
    network: NetworkConfig::testnet(),
    storage: StorageConfig::default(),
    enable_consensus: false,
    enable_rpc: true,
};
```

### Network Types

- **MainNet** - Production Neo network
- **TestNet** - Neo test network
- **PrivNet** - Private development network

## Development

### Project Structure

```
neo-rs/
├── crates/           # Individual crates
│   ├── core/         # Core types and utilities
│   ├── vm/           # Virtual machine
│   ├── consensus/    # Consensus implementation
│   ├── network/      # P2P networking
│   ├── ledger/       # Blockchain ledger
│   └── ...           # Other crates
├── node/             # Node binary
└── src/              # Library root
```

### Testing

```bash
# Run all tests
cargo test --workspace

# Run specific crate tests
cargo test -p neo-core

# Run integration tests
cargo test --features integration-tests
```

### Docker

Build and run with Docker:

```bash
# Build Docker image
docker build -t neo-rs .

# Run Neo node
docker run -p 10333:10333 -p 10332:10332 neo-rs
```

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -am 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Development Guidelines

- Follow Rust best practices and idioms
- Write comprehensive tests for new functionality
- Update documentation for public APIs
- Run `cargo fmt` and `cargo clippy` before submitting

## Performance

Neo-RS is designed for high performance:

- **Throughput**: Optimized for high transaction throughput
- **Latency**: Sub-millisecond block processing
- **Memory**: Efficient memory usage with smart caching
- **Storage**: Fast storage with RocksDB backend

## Security

- All cryptographic operations use well-audited libraries
- Memory-safe implementation eliminates common vulnerabilities
- Comprehensive input validation and error handling
- Regular security audits and dependency updates

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- [Neo Project](https://neo.org/) for the original protocol specification
- [Neo C# Implementation](https://github.com/neo-project/neo) for reference
- Rust community for excellent libraries and tools

## Support

- [GitHub Issues](https://github.com/r3e-network/neo-rs/issues) for bug reports
- [Discussions](https://github.com/r3e-network/neo-rs/discussions) for questions
- [Discord](https://discord.gg/neo) for community support

---

**Neo-RS** - High-performance Neo blockchain implementation in Rust 🦀