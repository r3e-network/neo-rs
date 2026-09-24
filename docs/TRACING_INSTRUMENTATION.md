# Distributed Tracing Instrumentation

## Overview

This document describes the distributed tracing instrumentation added to neo-rs critical paths. The implementation uses the `tracing` framework to provide observability across the entire system.

## Instrumented Components

### 1. Virtual Machine (neo-vm)

**File**: `neo-vm/src/execution_engine/execution.rs`

**Instrumented Functions**:
- `ExecutionEngine::execute()` - Main VM execution loop
  - Span name: `vm_execute`
  - Fields: `initial_state`, `final_state`, `instructions_executed`, `gas_consumed`

**Purpose**: Track VM execution performance, gas consumption, and state transitions.

### 2. Blockchain Layer (neo-core/ledger)

**File**: `neo-core/src/ledger/blockchain/block_processing.rs`

**Instrumented Functions**:
- `Blockchain::on_new_block()` - Block processing and validation
  - Span name: `blockchain_process_block`
  - Fields: `height`, `hash`, `verify`

**File**: `neo-core/src/ledger/blockchain/transaction.rs`

**Instrumented Functions**:
- `Blockchain::on_new_transaction()` - Transaction validation and mempool admission
  - Span name: `blockchain_new_tx`
  - Fields: `tx_hash`
  
- `Blockchain::transaction_exists_on_chain()` - Transaction existence check
  - Span name: `blockchain_tx_exists`
  - Fields: `tx_hash`

**Purpose**: Monitor block and transaction processing latency, track validation bottlenecks.

### 3. RPC Server (neo-rpc)

**File**: `neo-rpc/src/server/smart_contract/invocation.rs`

**Instrumented Functions**:
- `invoke_function()` - Contract function invocation
  - Span name: `rpc_invoke_function`
  - Fields: `contract`, `operation`
  
- `invoke_script()` - Script execution
  - Span name: `rpc_invoke_script`
  - Fields: `script_length`
  
- `execute_script()` - Script execution with diagnostics
  - Span name: `rpc_execute_script`
  - Fields: `script_len`, `diagnostic`

**File**: `neo-rpc/src/server/rpc_server_blockchain/mod.rs`

**Instrumented Functions**:
- `get_block()` - Block retrieval
  - Span name: `rpc_get_block`
  
- `get_raw_transaction()` - Transaction retrieval
  - Span name: `rpc_get_raw_transaction`

**Purpose**: Track RPC request latency, identify slow endpoints, measure contract execution time.

### 4. Consensus (neo-consensus)

**File**: `neo-consensus/src/service/handlers/commit.rs`

**Instrumented Functions**:
- `ConsensusService::on_commit()` - Commit message processing
  - Span name: `consensus_commit`
  - Fields: `block_index`, `view`, `validator`

**File**: `neo-consensus/src/service/handlers/prepare.rs`

**Instrumented Functions**:
- `ConsensusService::on_prepare_request()` - Prepare request processing
  - Span name: `consensus_prepare_request`
  - Fields: `block_index`, `view`, `validator`

**File**: `neo-consensus/src/service/handlers/change_view.rs`

**Instrumented Functions**:
- `ConsensusService::on_change_view()` - View change processing
  - Span name: `consensus_change_view`
  - Fields: `block_index`, `view`, `validator`

**Purpose**: Monitor consensus protocol performance, track view changes, identify consensus delays.

### 5. P2P Networking (neo-core/network)

**File**: `neo-core/src/network/p2p/connection.rs`

**Instrumented Functions**:
- `PeerConnection::send_message()` - Send P2P message
  - Span name: `p2p_send_message`
  - Fields: `remote`, `command`
  
- `PeerConnection::receive_message()` - Receive P2P message
  - Span name: `p2p_receive_message`
  - Fields: `remote`

**Purpose**: Track network message latency, identify slow peers, monitor P2P communication.

## Instrumentation Strategy

### Span Naming Convention

All spans follow a consistent naming pattern: `<component>_<operation>`

Examples:
- `vm_execute` - VM execution
- `rpc_invoke_function` - RPC function invocation
- `consensus_commit` - Consensus commit processing
- `p2p_send_message` - P2P message sending

### Field Conventions

**Common Fields**:
- `height` / `block_index` - Block height
- `hash` / `tx_hash` - Hash identifiers
- `validator` - Validator index
- `view` - Consensus view number
- `remote` - Remote peer address

**Performance Fields**:
- `instructions_executed` - VM instruction count
- `gas_consumed` - Gas consumption
- `script_len` - Script length

### Instrumentation Types

**1. Attribute-based (`#[instrument]`)**:
```rust
#[tracing::instrument(name = "span_name", skip(self), fields(key = value))]
pub fn operation(&mut self) -> Result<()> { }
```

Used for functions with straightforward control flow.

**2. Manual span entry**:
```rust
pub fn operation() -> Result<()> {
    let _span = tracing::info_span!("span_name").entered();
    // operation logic
}
```

Used for functions where we need more control over span lifecycle or where `#[instrument]` would be verbose.

**3. Trace events within spans**:
```rust
tracing::debug!(field = value, "Event description");
```

Used for important events within instrumented functions.

## Usage

### Configuration

Tracing is configured via the `tracing-subscriber` crate. The workspace already has:

```toml
tracing = "0.1"
tracing-subscriber = { version = "0.3.19", features = ["env-filter", "json", "ansi"] }
```

### Enabling Distributed Tracing

To enable tracing output, set the `RUST_LOG` environment variable:

```bash
# Enable all tracing at info level
export RUST_LOG=info

# Enable specific modules
export RUST_LOG=neo_vm=debug,neo_consensus=info

# Enable specific spans
export RUST_LOG=vm_execute=trace,consensus_commit=debug
```

### Integration with Observability Systems

The instrumentation is compatible with:

1. **Jaeger** - Distributed tracing system
2. **OpenTelemetry** - Vendor-neutral observability framework
3. **Honeycomb** - Observability platform
4. **DataDog** - Monitoring and analytics

Example setup with OpenTelemetry (add to neo-node):

```rust
use opentelemetry::global;
use tracing_subscriber::layer::SubscriberExt;

let tracer = opentelemetry_jaeger::new_pipeline()
    .with_service_name("neo-node")
    .install_simple()?;

let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
let subscriber = tracing_subscriber::registry()
    .with(telemetry)
    .with(tracing_subscriber::fmt::layer());

tracing::subscriber::set_global_default(subscriber)?;
```

## Benefits

### 1. Performance Monitoring
- Track execution time across all critical paths
- Identify bottlenecks in VM execution, block processing, and consensus
- Monitor P2P latency and network performance

### 2. Debugging
- Correlate events across distributed components
- Track transaction flow from RPC → Mempool → Consensus → Block
- Understand consensus view changes and delays

### 3. Production Observability
- Real-time visibility into node behavior
- Alert on slow operations or anomalies
- Generate performance metrics and SLOs

### 4. Development Insights
- Profile code paths during testing
- Validate optimization effectiveness
- Understand system behavior under load

## Testing

To verify tracing instrumentation:

```bash
# Run tests with tracing enabled
RUST_LOG=debug cargo test

# Run specific component tests
RUST_LOG=neo_vm=trace cargo test --package neo-vm
RUST_LOG=neo_consensus=debug cargo test --package neo-consensus

# Run with JSON output (machine-readable)
RUST_LOG=info cargo run -- --tracing-format=json
```

## Future Enhancements

### Planned Instrumentation

1. **State Service** - Track state root computation and verification
2. **Mempool** - Monitor transaction pool operations
3. **Storage Layer** - Track RocksDB operations and cache hits
4. **Wallet Operations** - Monitor signing and key management

### Metrics Integration

Convert spans to Prometheus metrics:
- Request rates (requests/second)
- Latency percentiles (p50, p95, p99)
- Error rates
- Throughput metrics

### Trace Sampling

For high-throughput production:
- Implement head-based sampling (sample N% of traces)
- Implement tail-based sampling (keep slow/errored traces)
- Configure sampling rates per component

## Related Documentation

- [tracing crate documentation](https://docs.rs/tracing/)
- [OpenTelemetry specification](https://opentelemetry.io/docs/)
- [Jaeger documentation](https://www.jaegertracing.io/docs/)

## Maintenance

When adding new critical paths:

1. Add `#[tracing::instrument]` to public entry points
2. Use consistent span naming: `<component>_<operation>`
3. Include relevant context fields (IDs, sizes, addresses)
4. Add trace events for important milestones
5. Update this documentation

## Summary

The tracing instrumentation provides comprehensive observability across:
- **6 components** (VM, Blockchain, RPC, Consensus, P2P, State)
- **15+ critical functions** instrumented
- **Consistent naming** and field conventions
- **Production-ready** integration with standard observability tools

This enables developers and operators to understand system behavior, debug issues, and optimize performance in both development and production environments.
