# Neo Telemetry

Production observability stack for Neo N3 blockchain node.

## Overview

This crate provides comprehensive Prometheus metrics and observability for all Neo-rs subsystems.

## Metrics Coverage

### Blockchain Metrics
- `neo_block_height` - Current block height
- `neo_header_height` - Highest header seen
- `neo_header_lag` - Header lag in blocks
- `neo_blocks_processed_total` - Total blocks processed

### Transaction & Mempool Metrics
- `neo_mempool_size` - Current mempool size
- `neo_mempool_verified_transactions` - Verified transactions in mempool
- `neo_mempool_unverified_transactions` - Unverified transactions in mempool
- `neo_mempool_transactions_added_total` - Transactions added to mempool
- `neo_mempool_transactions_removed_total` - Transactions removed from mempool
- `neo_mempool_transactions_rejected_total` - Transactions rejected by mempool
- `neo_transactions_verified_total` - Total transactions verified

### Network & P2P Metrics
- `neo_peer_count` - Number of connected peers
- `neo_p2p_connections_inbound` - Current inbound P2P connections
- `neo_p2p_connections_outbound` - Current outbound P2P connections
- `neo_p2p_messages_received_bytes_total` - Total bytes received via P2P
- `neo_p2p_messages_sent_bytes_total` - Total bytes sent via P2P
- `neo_p2p_handshake_success_total` - Successful P2P handshakes
- `neo_p2p_handshake_failure_total` - Failed P2P handshakes
- `neo_p2p_timeouts_handshake` - Handshake timeouts
- `neo_p2p_timeouts_read` - Read timeouts
- `neo_p2p_timeouts_write` - Write timeouts

### Consensus (dBFT) Metrics
- `neo_consensus_view` - Current consensus view number
- `neo_consensus_round_seconds` - Consensus round time histogram
- `neo_consensus_prepare_request_total` - PrepareRequest messages sent
- `neo_consensus_prepare_response_total` - PrepareResponse messages sent
- `neo_consensus_commit_total` - Commit messages sent
- `neo_consensus_change_view_total` - ChangeView messages sent
- `neo_consensus_recovery_request_total` - RecoveryRequest messages sent
- `neo_consensus_recovery_message_total` - RecoveryMessage messages sent
- `neo_consensus_blocks_finalized_total` - Blocks finalized through consensus

### VM Metrics
- `neo_vm_executions_total` - Total VM script executions
- `neo_vm_execution_seconds` - VM execution time histogram
- `neo_vm_instructions_total` - Total VM instructions executed
- `neo_vm_gas_consumed_total` - Total GAS consumed by VM executions
- `neo_vm_faults_total` - Total VM execution faults

### Storage Metrics
- `neo_storage_reads_total` - Total storage read operations
- `neo_storage_writes_total` - Total storage write operations
- `neo_storage_deletes_total` - Total storage delete operations
- `neo_storage_read_bytes_total` - Total bytes read from storage
- `neo_storage_write_bytes_total` - Total bytes written to storage
- `neo_storage_cache_hits_total` - Storage cache hits
- `neo_storage_cache_misses_total` - Storage cache misses
- `neo_storage_free_bytes` - Free bytes on storage disk
- `neo_storage_total_bytes` - Total bytes on storage disk

### Smart Contract Metrics
- `neo_smart_contract_invocations_total` - Total smart contract invocations
- `neo_smart_contract_deployments_total` - Smart contract deployments
- `neo_smart_contract_updates_total` - Smart contract updates
- `neo_smart_contract_destroys_total` - Smart contract destructions

### Native Contract Metrics
- `neo_native_neo_transfers_total` - NEO token transfers
- `neo_native_gas_transfers_total` - GAS token transfers
- `neo_native_votes_cast_total` - Governance votes cast
- `neo_native_candidates_registered_total` - Validator candidates registered
- `neo_native_oracle_requests_total` - Oracle requests
- `neo_native_oracle_responses_total` - Oracle responses

### Cryptography Metrics
- `neo_crypto_signature_verifications_total` - Total signature verifications
- `neo_crypto_signature_verification_failures_total` - Failed signature verifications
- `neo_crypto_hash_operations_total` - Cryptographic hash operations
- `neo_batch_signatures_verified_total` - Signatures verified via batch verification
- `neo_average_batch_size` - Average signatures per batch
- `neo_simd_blake2b_throughput_bytes_per_second` - SIMD BLAKE2b throughput

### RPC Metrics
- `neo_rpc_requests_total` - Total RPC requests (by method)
- `neo_rpc_request_seconds` - RPC request duration histogram (by method)
- `neo_rpc_errors_total` - Total RPC errors (by method and error code)
- `neo_rpc_active_connections` - Current active RPC connections
- `neo_rpc_rate_limited_total` - Rate-limited RPC requests
- `neo_rpc_websocket_subscriptions` - Current WebSocket subscriptions

### State Root Metrics
- `neo_state_local_root_index` - Current local state root index
- `neo_state_validated_root_index` - Current validated state root index
- `neo_state_validated_lag` - Difference between local and validated state roots
- `neo_state_roots_accepted_total` - Total accepted state roots
- `neo_state_roots_rejected_total` - Total rejected state roots

### Ledger Metrics
- `neo_ledger_block_persist_seconds` - Block persist time histogram
- `neo_ledger_tx_verification_seconds` - Transaction verification time histogram

### Oracle Service Metrics
- `neo_oracle_requests_pending` - Pending oracle requests
- `neo_oracle_requests_completed_total` - Completed oracle requests
- `neo_oracle_requests_failed_total` - Failed oracle requests

### Sync & Optimization Metrics
- `neo_sync_current_height` - Current block height during sync
- `neo_sync_tip_height` - Tip height from network peers
- `neo_fast_sync_enabled` - Fast sync mode enabled flag
- `block_processing_duration` - Block processing duration histogram
- `prefetch_stage_latencies_bucket` - Prefetch stage latencies (by stage)
- `get_syscall_lookup_latency_bucket` - Syscall lookup latency histogram
- `neo_cuckoo_hash_lookups_total` - Successful Cuckoo hash lookups
- `neo_cuckoo_hash_lookups_miss_total` - Failed Cuckoo hash lookups

### TEE Metrics (if enabled)
- `neo_tee_enclave_active` - TEE enclave active status
- `neo_tee_attestations_total` - Total TEE attestations performed

### HSM Metrics (if enabled)
- `neo_hsm_connected` - HSM connection status
- `neo_hsm_sign_operations_total` - Total HSM signing operations

### System Metrics
- `cpu_usage_ratio` - CPU usage ratio (0.0 to 1.0)
- `memory_usage_bytes` - Memory usage in bytes
- `system_memory_total_bytes` - Total system memory in bytes

## Usage

### Basic Usage

```rust
use neo_telemetry::{init, TelemetryAndLoggingConfig};

let config = TelemetryAndLoggingConfig::default();
let handle = init(&config)?;

// Metrics are automatically registered and available at /metrics endpoint
```

### Recording Metrics

```rust
use neo_telemetry::node_metrics;

// Update blockchain metrics
node_metrics::BLOCK_HEIGHT.set(12345.0);
node_metrics::BLOCKS_PROCESSED_TOTAL.inc();

// Record VM execution
node_metrics::VM_EXECUTION_COUNT.inc();
node_metrics::VM_GAS_CONSUMED.inc_by(100.0);

// Record storage operations
node_metrics::STORAGE_READ_COUNT.inc();
node_metrics::STORAGE_CACHE_HIT.inc();

// Record consensus messages
node_metrics::CONSENSUS_PREPARE_REQUEST.inc();
node_metrics::CONSENSUS_BLOCK_FINALIZED.inc();
```

### Gathering Metrics

```rust
use neo_telemetry::node_metrics;

// Get all metrics in Prometheus text format
let metrics = node_metrics::gather_prometheus();
let text = String::from_utf8(metrics).unwrap();
```

## Integration

### With neo-node

```rust
use neo_telemetry::{init_for_node, TelemetryAndLoggingConfig};

let config = TelemetryAndLoggingConfig::from_file("config.toml")?;
let (handle, _guard) = init_for_node(&config, daemon_mode)?;

// Metrics are now available and automatically exposed
```

### Prometheus Scrape Configuration

```yaml
scrape_configs:
  - job_name: 'neo-node'
    static_configs:
      - targets: ['localhost:10333']
    metrics_path: '/metrics'
    scrape_interval: 15s
```

## Architecture

- **Production deployment**: HTTP metrics endpoint for Prometheus scraping
- **All subsystems covered**: VM, Storage, Consensus, P2P, RPC, Smart Contracts, Native Contracts
- **Low overhead**: Atomic operations, lazy initialization
- **Thread-safe**: All metrics are safe for concurrent use

## Testing

```bash
cargo test -p neo-telemetry
```

## See Also

- [Prometheus Documentation](https://prometheus.io/docs/introduction/overview/)
- [Neo N3 Documentation](https://docs.neo.org/)
