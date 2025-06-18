<p align="center">
  <img src="https://neo3.azureedge.net/images/logo%20files-dark.svg" width="250px" alt="neo-logo">
</p>

<h3 align="center">Rust implementation of the Neo blockchain protocol.</h3>

<p align="center">
   A modern distributed network for the Smart Economy.
</p>

# Neo-RS

Neo-RS is a production-ready Rust implementation of the Neo N3 blockchain protocol. This project provides a high-performance, memory-safe alternative to the C# implementation with full Neo N3 network compatibility.

## Features

✅ **Complete Neo N3 Node Implementation**
- Full P2P network connectivity to Neo MainNet/TestNet
- Block synchronization and processing  
- Comprehensive RPC server with 30+ Neo N3 API methods
- Graceful shutdown coordination

✅ **Production-Ready Components**
- **Cryptography**: Complete cryptographic suite (ECDSA, Ed25519, SHA256, RIPEMD160, etc.)
- **VM**: Full Neo virtual machine with C# compatibility
- **Smart Contracts**: Complete smart contract execution environment
- **Network**: Robust P2P networking with message validation
- **Persistence**: RocksDB-based storage with comprehensive backup features
- **Consensus**: dBFT consensus implementation (disabled by default)

## Quick Start

### Running the Neo Node

```bash
# Run on TestNet (recommended for testing)
cargo run --bin neo-node -- --testnet

# Run on MainNet
cargo run --bin neo-node -- --mainnet

# Custom ports
cargo run --bin neo-node -- --testnet --rpc-port 20332 --p2p-port 20333
```

### Node Status Monitoring

The node provides comprehensive status reporting every 30 seconds:
- Blockchain height and sync speed
- Network peer connections and statistics  
- Synchronization health and progress
- Data transfer metrics

## Project Structure

The project is organized as a Rust workspace with multiple crates:

```
neo-rs/
├── crates/                  # Rust crates (modules)
│   ├── core/                # Core functionality
│   ├── cryptography/        # Cryptographic implementations
│   ├── io/                  # IO operations and data structures
│   ├── ledger/              # Blockchain state management
│   ├── network/             # P2P protocol implementation
│   ├── persistence/         # State access interfaces
│   ├── plugins/             # Extension interfaces
│   ├── smart_contract/      # Smart contract related functionality
│   ├── vm/                  # Virtual machine implementation
│   └── wallets/             # Wallet and account implementation
├── cli/                     # Command-line interface
└── docs/                    # Documentation
    ├── modules/             # Module-specific documentation
    └── conversion/          # Conversion tracking and notes
```

## Building

```bash
# Build the complete node
cargo build --release

# Build just the node executable
cargo build --bin neo-node --release
```

## Configuration

The node supports flexible configuration:

```bash
# Create config file
cp neo-config.toml.example neo-config.toml

# Edit configuration as needed
# The node will auto-create default config if none exists
```

## Testing

```bash
# Run all tests
cargo test

# Run network connectivity tests
cargo test network_connectivity

# Run integration tests
cargo test integration_tests

# Run specific module tests
cargo test -p neo-vm
cargo test -p neo-cryptography
```

## Performance

Neo-RS is designed for high performance:
- **Memory Safe**: Rust's ownership system prevents memory leaks and data races
- **High Throughput**: Optimized for concurrent block processing
- **Low Resource Usage**: Efficient memory and CPU utilization
- **Fast Sync**: Parallel block downloading and validation

## Network Compatibility

Fully compatible with Neo N3 network protocol:
- ✅ Connects to official Neo MainNet and TestNet
- ✅ Supports all Neo N3 message types
- ✅ Compatible with Neo C# nodes
- ✅ Validates blocks and transactions according to Neo N3 rules

## Development

### Architecture

The codebase follows Neo's modular architecture:
- **Core**: Fundamental types and utilities
- **VM**: Neo Virtual Machine implementation  
- **Network**: P2P networking and consensus
- **Persistence**: Blockchain data storage
- **Smart Contract**: Contract execution environment

### Contributing

Contributions are welcome! The codebase maintains compatibility with Neo N3 protocol specifications.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
