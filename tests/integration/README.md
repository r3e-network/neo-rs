# Neo-RS Integration Tests

This directory contains comprehensive integration tests for the Neo blockchain implementation in Rust. These tests verify that all major components work correctly together in realistic scenarios.

## Test Categories

### 1. P2P Networking Tests (`p2p_integration_test.rs`)
Tests the peer-to-peer networking layer including:
- Peer discovery and connection management
- Handshake protocol implementation
- Message propagation and routing
- Connection resilience and recovery
- Protocol violation handling
- DoS protection and rate limiting

### 2. Consensus Tests (`consensus_integration_test.rs`)
Tests the dBFT consensus mechanism including:
- Multi-validator consensus rounds
- Byzantine fault tolerance (handling malicious validators)
- View change mechanisms
- Recovery and catch-up protocols
- Performance under various network conditions
- State persistence across restarts

### 3. Block Synchronization Tests (`block_sync_integration_test.rs`)
Tests blockchain synchronization including:
- Initial block download from genesis
- Header-first synchronization strategy
- Parallel block downloading from multiple peers
- Chain reorganization handling
- Checkpoint-based fast synchronization
- Recovery from interrupted sync
- Slow peer handling

### 4. Execution Tests (`execution_integration_test.rs`)
Tests transaction and block execution including:
- Transaction validation and execution
- Block processing and state transitions
- Smart contract deployment and invocation
- Gas calculation and resource limits
- State rollback on execution failure
- Concurrent transaction processing

### 5. End-to-End Tests (`end_to_end_test.rs`)
Tests complete system functionality including:
- Full network simulation with multiple nodes
- Complete transaction lifecycle from submission to inclusion
- Cross-node state consistency verification
- High-throughput transaction processing
- Network fault tolerance and recovery
- Smart contract state consistency

## Running the Tests

### Run All Integration Tests
```bash
cargo test --test integration_tests --features integration-tests
```

### Run Specific Test Category
```bash
# P2P networking tests
cargo test --test integration_tests p2p -- --nocapture

# Consensus tests
cargo test --test integration_tests consensus -- --nocapture

# Block synchronization tests
cargo test --test integration_tests block_sync -- --nocapture

# Execution tests
cargo test --test integration_tests execution -- --nocapture

# End-to-end tests
cargo test --test integration_tests end_to_end -- --nocapture
```

### Run with Debug Logging
```bash
RUST_LOG=debug cargo test --test integration_tests -- --nocapture
```

### Run Single Test
```bash
cargo test --test integration_tests test_full_consensus_network -- --exact --nocapture
```

## Test Environment

### Temporary Data
Tests create temporary blockchain data in `/tmp/neo-test-*` directories which are automatically cleaned up after test completion.

### Port Usage
Tests use ports in the range 30000-60000 to avoid conflicts with production Neo nodes. Each test uses a unique set of ports allocated by the test framework.

### Performance
Some tests simulate realistic network conditions and may take several minutes to complete. The default timeout is 5 minutes per test.

## Writing New Tests

When adding new integration tests:

1. Place them in the appropriate category file or create a new category
2. Use the `test_utils` module for common functionality
3. Clean up all resources (abort spawned tasks, remove temp directories)
4. Use descriptive test names that explain what is being tested
5. Add appropriate log statements for debugging
6. Document any special requirements or assumptions

## Debugging Failed Tests

1. Run with `--nocapture` to see all output
2. Enable debug logging with `RUST_LOG=debug`
3. Check `/tmp/neo-test-*` directories for blockchain data
4. Use shorter timeouts for faster iteration
5. Run individual tests in isolation to identify issues

## Known Limitations

- Tests require significant system resources (CPU, memory, disk)
- Some tests may be flaky under heavy system load
- Network tests may fail in restricted environments (containers, CI)
- Long-running tests may timeout in resource-constrained environments

## Contributing

When contributing new integration tests:
- Ensure tests are deterministic and reproducible
- Avoid hardcoded delays; use proper synchronization
- Clean up all resources to prevent interference
- Document any external dependencies or requirements
- Ensure tests work in CI environment