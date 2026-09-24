# Metrics Integration Guide

This guide shows how to integrate the expanded Prometheus metrics into Neo-rs subsystems.

## Quick Start

All metrics are defined as `LazyLock` statics in `neo_telemetry::node_metrics` and are automatically registered on first access.

```rust
use neo_telemetry::node_metrics;

// Simply increment or set the metric - no initialization needed
node_metrics::BLOCK_HEIGHT.set(12345.0);
node_metrics::BLOCKS_PROCESSED_TOTAL.inc();
```

## Integration by Subsystem

### neo-vm Integration

Add to VM execution paths:

```rust
use neo_telemetry::node_metrics;

pub fn execute(&mut self) -> Result<VMState, VMError> {
    let start = std::time::Instant::now();
    node_metrics::VM_EXECUTION_COUNT.inc();
    
    let result = self.execute_internal();
    
    let elapsed = start.elapsed().as_secs_f64();
    node_metrics::VM_EXECUTION_TIME.observe(elapsed);
    
    match &result {
        Ok(_) => {
            node_metrics::VM_INSTRUCTION_COUNT.inc_by(self.instructions_executed as f64);
            node_metrics::VM_GAS_CONSUMED.inc_by(self.gas_consumed as f64);
        }
        Err(_) => {
            node_metrics::VM_FAULTS.inc();
        }
    }
    
    result
}
```

### neo-storage Integration

Add to storage backend implementations:

```rust
use neo_telemetry::node_metrics;

impl StorageBackend for RocksDbBackend {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let result = self.db.get(key)?;
        
        node_metrics::STORAGE_READ_COUNT.inc();
        if let Some(ref value) = result {
            node_metrics::STORAGE_READ_BYTES.inc_by(value.len() as f64);
            node_metrics::STORAGE_CACHE_HIT.inc();
        } else {
            node_metrics::STORAGE_CACHE_MISS.inc();
        }
        
        Ok(result)
    }
    
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.db.put(key, value)?;
        node_metrics::STORAGE_WRITE_COUNT.inc();
        node_metrics::STORAGE_WRITE_BYTES.inc_by(value.len() as f64);
        Ok(())
    }
    
    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.db.delete(key)?;
        node_metrics::STORAGE_DELETE_COUNT.inc();
        Ok(())
    }
}
```

### neo-consensus Integration

Add to consensus message handlers:

```rust
use neo_telemetry::node_metrics;

impl ConsensusService {
    fn send_prepare_request(&mut self) -> Result<()> {
        // ... send logic ...
        node_metrics::CONSENSUS_PREPARE_REQUEST.inc();
        Ok(())
    }
    
    fn send_prepare_response(&mut self) -> Result<()> {
        // ... send logic ...
        node_metrics::CONSENSUS_PREPARE_RESPONSE.inc();
        Ok(())
    }
    
    fn send_commit(&mut self) -> Result<()> {
        // ... send logic ...
        node_metrics::CONSENSUS_COMMIT.inc();
        Ok(())
    }
    
    fn send_change_view(&mut self) -> Result<()> {
        // ... send logic ...
        node_metrics::CONSENSUS_CHANGE_VIEW.inc();
        Ok(())
    }
    
    fn finalize_block(&mut self) -> Result<()> {
        // ... finalization logic ...
        node_metrics::CONSENSUS_BLOCK_FINALIZED.inc();
        Ok(())
    }
}
```

### neo-p2p Integration

Add to P2P connection manager:

```rust
use neo_telemetry::node_metrics;

impl P2PManager {
    fn on_connection_established(&mut self, inbound: bool) {
        if inbound {
            self.inbound_count += 1;
            node_metrics::P2P_CONNECTIONS_INBOUND.set(self.inbound_count as f64);
        } else {
            self.outbound_count += 1;
            node_metrics::P2P_CONNECTIONS_OUTBOUND.set(self.outbound_count as f64);
        }
    }
    
    fn on_handshake_complete(&mut self, success: bool) {
        if success {
            node_metrics::P2P_HANDSHAKE_SUCCESS.inc();
        } else {
            node_metrics::P2P_HANDSHAKE_FAILURE.inc();
        }
    }
    
    fn on_message_received(&mut self, bytes: usize) {
        node_metrics::P2P_MESSAGES_RECEIVED_BYTES.inc_by(bytes as f64);
    }
    
    fn on_message_sent(&mut self, bytes: usize) {
        node_metrics::P2P_MESSAGES_SENT_BYTES.inc_by(bytes as f64);
    }
}
```

### neo-crypto Integration

Add to signature verification:

```rust
use neo_telemetry::node_metrics;

pub fn verify_signature(
    message: &[u8],
    signature: &[u8],
    public_key: &[u8]
) -> Result<bool> {
    node_metrics::CRYPTO_SIGNATURE_VERIFICATIONS.inc();
    
    let result = verify_signature_internal(message, signature, public_key);
    
    if result.is_err() || !result.as_ref().unwrap() {
        node_metrics::CRYPTO_SIGNATURE_VERIFICATION_FAILURES.inc();
    }
    
    result
}

pub fn hash(data: &[u8]) -> Vec<u8> {
    node_metrics::CRYPTO_HASH_OPERATIONS.inc();
    hash_internal(data)
}
```

### neo-core Smart Contract Integration

Add to application engine:

```rust
use neo_telemetry::node_metrics;

impl ApplicationEngine {
    pub fn invoke(&mut self, script: &[u8]) -> Result<()> {
        node_metrics::SMART_CONTRACT_INVOCATIONS.inc();
        self.load_script(script)?;
        self.execute()
    }
    
    pub fn deploy_contract(&mut self, nef: &[u8], manifest: &str) -> Result<()> {
        node_metrics::SMART_CONTRACT_DEPLOY.inc();
        // ... deployment logic ...
        Ok(())
    }
    
    pub fn update_contract(&mut self, nef: &[u8], manifest: &str) -> Result<()> {
        node_metrics::SMART_CONTRACT_UPDATE.inc();
        // ... update logic ...
        Ok(())
    }
    
    pub fn destroy_contract(&mut self) -> Result<()> {
        node_metrics::SMART_CONTRACT_DESTROY.inc();
        // ... destroy logic ...
        Ok(())
    }
}
```

### Native Contract Integration

Add to NEO and GAS token transfers:

```rust
use neo_telemetry::node_metrics;

impl NeoToken {
    fn transfer(&mut self, from: &UInt160, to: &UInt160, amount: i64) -> Result<bool> {
        node_metrics::NATIVE_CONTRACT_NEO_TRANSFER.inc();
        // ... transfer logic ...
        Ok(true)
    }
    
    fn vote(&mut self, account: &UInt160, vote_to: &ECPoint) -> Result<bool> {
        node_metrics::NATIVE_CONTRACT_VOTE_CAST.inc();
        // ... voting logic ...
        Ok(true)
    }
    
    fn register_candidate(&mut self, pubkey: &ECPoint) -> Result<bool> {
        node_metrics::NATIVE_CONTRACT_CANDIDATE_REGISTERED.inc();
        // ... registration logic ...
        Ok(true)
    }
}

impl GasToken {
    fn transfer(&mut self, from: &UInt160, to: &UInt160, amount: i64) -> Result<bool> {
        node_metrics::NATIVE_CONTRACT_GAS_TRANSFER.inc();
        // ... transfer logic ...
        Ok(true)
    }
}
```

### Oracle Service Integration

Add to oracle request handler:

```rust
use neo_telemetry::node_metrics;

impl OracleService {
    pub fn request(&mut self, url: &str, filter: &str) -> Result<u64> {
        node_metrics::NATIVE_CONTRACT_ORACLE_REQUEST.inc();
        node_metrics::ORACLE_REQUESTS_PENDING.inc(1.0);
        // ... request logic ...
        Ok(request_id)
    }
    
    pub fn process_response(&mut self, request_id: u64, response: &[u8]) -> Result<()> {
        node_metrics::NATIVE_CONTRACT_ORACLE_RESPONSE.inc();
        node_metrics::ORACLE_REQUESTS_PENDING.dec(1.0);
        node_metrics::ORACLE_REQUESTS_COMPLETED.inc();
        // ... response processing ...
        Ok(())
    }
    
    pub fn on_request_failed(&mut self, request_id: u64) {
        node_metrics::ORACLE_REQUESTS_PENDING.dec(1.0);
        node_metrics::ORACLE_REQUESTS_FAILED.inc();
    }
}
```

### neo-rpc Integration

Add to RPC server:

```rust
use neo_telemetry::node_metrics;

impl RpcServer {
    async fn handle_request(&self, method: &str) -> Result<Value> {
        node_metrics::RPC_ACTIVE_CONNECTIONS.inc(1.0);
        
        let start = std::time::Instant::now();
        let result = self.handle_method(method).await;
        let elapsed = start.elapsed().as_secs_f64();
        
        // Record metrics (existing metrics module has these)
        self.metrics.rpc_requests.with_label_values(&[method]).inc();
        self.metrics.rpc_request_duration
            .with_label_values(&[method])
            .observe(elapsed);
        
        if let Err(ref e) = result {
            let error_code = format!("{}", e.code());
            self.metrics.rpc_errors
                .with_label_values(&[method, &error_code])
                .inc();
        }
        
        node_metrics::RPC_ACTIVE_CONNECTIONS.dec(1.0);
        result
    }
    
    fn on_rate_limited(&self) {
        node_metrics::RPC_RATE_LIMITED.inc();
    }
    
    fn on_websocket_subscribe(&self) {
        node_metrics::RPC_WEBSOCKET_SUBSCRIPTIONS.inc(1.0);
    }
    
    fn on_websocket_unsubscribe(&self) {
        node_metrics::RPC_WEBSOCKET_SUBSCRIPTIONS.dec(1.0);
    }
}
```

### Mempool Integration

Add to transaction pool:

```rust
use neo_telemetry::node_metrics;

impl MemoryPool {
    pub fn add_transaction(&mut self, tx: Transaction) -> Result<bool> {
        let result = self.try_add_transaction(tx)?;
        
        if result {
            node_metrics::MEMPOOL_TX_ADDED.inc();
            self.update_metrics();
        } else {
            node_metrics::MEMPOOL_TX_REJECTED.inc();
        }
        
        Ok(result)
    }
    
    pub fn remove_transaction(&mut self, hash: &UInt256) -> Option<Transaction> {
        let tx = self.transactions.remove(hash);
        if tx.is_some() {
            node_metrics::MEMPOOL_TX_REMOVED.inc();
            self.update_metrics();
        }
        tx
    }
    
    fn update_metrics(&self) {
        node_metrics::MEMPOOL_VERIFIED_TX.set(self.verified.len() as f64);
        node_metrics::MEMPOOL_UNVERIFIED_TX.set(self.unverified.len() as f64);
    }
}
```

### Ledger Integration

Add to blockchain persistence:

```rust
use neo_telemetry::node_metrics;

impl Blockchain {
    pub async fn persist_block(&mut self, block: &Block) -> Result<()> {
        let start = std::time::Instant::now();
        
        // ... persistence logic ...
        
        let elapsed = start.elapsed().as_secs_f64();
        node_metrics::LEDGER_BLOCK_PERSIST_TIME.observe(elapsed);
        
        Ok(())
    }
    
    pub fn verify_transaction(&self, tx: &Transaction) -> Result<()> {
        let start = std::time::Instant::now();
        
        let result = self.verify_transaction_internal(tx);
        
        let elapsed = start.elapsed().as_secs_f64();
        node_metrics::LEDGER_TX_VERIFICATION_TIME.observe(elapsed);
        
        result
    }
}
```

## Performance Considerations

- All metrics use atomic operations (no locks)
- Metrics are initialized lazily on first access
- Negligible overhead: ~10-20ns per increment/set operation
- Histogram observations: ~50-100ns per sample

## Conditional Compilation

For zero-overhead in non-production builds:

```rust
#[cfg(feature = "metrics")]
use neo_telemetry::node_metrics;

#[inline(always)]
fn record_metric_if_enabled() {
    #[cfg(feature = "metrics")]
    node_metrics::BLOCK_HEIGHT.set(height as f64);
}
```

## Testing

Test that metrics are recorded correctly:

```rust
#[test]
fn test_metrics_recorded() {
    let initial = neo_telemetry::node_metrics::BLOCKS_PROCESSED_TOTAL.get();
    
    process_block(&block);
    
    let after = neo_telemetry::node_metrics::BLOCKS_PROCESSED_TOTAL.get();
    assert!(after > initial);
}
```
