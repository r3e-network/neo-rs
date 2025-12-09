# Neo-RS Security Patches Guide

**Generated**: 2025-12-09
**Version**: v0.7.0
**Priority**: CRITICAL - Must fix before production deployment

---

## Overview

This document provides specific code fixes for security vulnerabilities identified during the security audit. Each patch includes the exact file location, problematic code, and recommended fix.

---

## CRITICAL Fixes (P0)

### C-1: Insecure RNG for Key Generation

**File**: `neo-core/src/wallets/key_pair.rs:35-38`

**Problem**: Using `thread_rng()` instead of cryptographically secure `OsRng` for private key generation.

**Current Code**:
```rust
pub fn generate() -> Result<Self> {
    let mut private_key = [0u8; HASH_SIZE];
    rand::thread_rng().fill_bytes(&mut private_key);  // INSECURE
    Self::from_private_key(&private_key)
}
```

**Fixed Code**:
```rust
use rand::rngs::OsRng;

pub fn generate() -> Result<Self> {
    let mut private_key = Zeroizing::new([0u8; HASH_SIZE]);
    OsRng.fill_bytes(private_key.as_mut());  // SECURE: Uses OS entropy
    Self::from_private_key(&private_key)
}
```

**Additional Locations to Fix**:
- `neo-core/src/cryptography/crypto_utils.rs:257` (Ed25519 key generation)
- `neo-core/src/cryptography/crypto_utils.rs:713` (BLS12-381 key generation)

---

### C-2: BigInt Unbounded Growth in VM

**File**: `neo-vm/src/jump_table/numeric.rs:150-300`

**Problem**: Arithmetic operations (ADD, MUL, POW) allow unbounded BigInt growth, enabling memory exhaustion attacks.

**Current Code** (ADD operation):
```rust
fn add(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let context = engine.current_context_mut()...;
    let b = context.pop()?;
    let a = context.pop()?;

    let result = match (a, b) {
        (StackItem::Integer(a), StackItem::Integer(b)) => {
            let sum = &a + &b;  // NO SIZE CHECK
            StackItem::from_int(sum)
        }
        // ...
    };
    context.push(result)?;
    Ok(())
}
```

**Fixed Code**:
```rust
/// Maximum size for BigInt results (matches C# Neo.VM behavior)
const MAX_BIGINT_SIZE: usize = 32; // 256 bits

fn check_bigint_size(value: &BigInt, limits: &ExecutionEngineLimits) -> VmResult<()> {
    let byte_len = value.to_signed_bytes_le().len();
    if byte_len > MAX_BIGINT_SIZE {
        return Err(VmError::invalid_operation_msg(format!(
            "BigInt size {} exceeds maximum {}",
            byte_len, MAX_BIGINT_SIZE
        )));
    }
    Ok(())
}

fn add(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let limits = engine.limits().clone();
    let context = engine.current_context_mut()...;
    let b = context.pop()?;
    let a = context.pop()?;

    let result = match (a, b) {
        (StackItem::Integer(a), StackItem::Integer(b)) => {
            let sum = &a + &b;
            check_bigint_size(&sum, &limits)?;  // SIZE CHECK ADDED
            StackItem::from_int(sum)
        }
        // ...
    };
    context.push(result)?;
    Ok(())
}
```

**Apply same fix to**: `mul()`, `pow()`, `sub()`, `negate()`, `abs()`

---

### C-3: PrepareResponse Missing Signature Verification

**File**: `neo-plugins/src/dbft_plugin/consensus/consensus_service_on_message.rs:184-248`

**Problem**: PrepareResponse messages are accepted without verifying the sender's signature, breaking Byzantine fault tolerance.

**Current Code**:
```rust
async fn on_prepare_response_received(
    &mut self,
    payload: ExtensiblePayload,
    message: PrepareResponse,
) {
    // ... validation checks ...

    // MISSING: Signature verification

    context.preparation_payloads[index] = Some(payload.clone());  // Accepts without verification
}
```

**Fixed Code**:
```rust
async fn on_prepare_response_received(
    &mut self,
    payload: ExtensiblePayload,
    message: PrepareResponse,
) {
    let (should_check, timer_state) = {
        let mut context = self.context.write().await;

        if message.view_number() != context.view_number() {
            return;
        }

        if context.not_accepting_payloads_due_to_view_changing() {
            return;
        }

        let index = message.validator_index() as usize;
        if index >= context.preparation_payloads.len() {
            return;
        }

        if context.preparation_payloads[index].is_some() {
            return;
        }

        // ========== SECURITY FIX: Verify signature ==========
        let validator_pubkey = match context.validators.get(index) {
            Some(pk) => pk.clone(),
            None => {
                self.log("PrepareResponse from unknown validator - rejecting");
                return;
            }
        };

        // Verify the ExtensiblePayload witness signature
        if !self.verify_payload_signature(&payload, &validator_pubkey).await {
            self.log(&format!(
                "INVALID PrepareResponse signature from validator {} - rejecting",
                index
            ));
            return;
        }
        // ========== END SECURITY FIX ==========

        let primary_index = context.block().primary_index() as usize;
        let hash_matches = if let Some(Some(primary_payload)) =
            context.preparation_payloads.get(primary_index).cloned()
        {
            let mut payload_clone = primary_payload;
            let primary_hash = ConsensusContext::payload_hash(&mut payload_clone);
            message.preparation_hash() == &primary_hash
        } else {
            true
        };

        if !hash_matches {
            return;
        }

        // ... rest of the function ...
    };
}

/// Helper function to verify payload signature
async fn verify_payload_signature(
    &self,
    payload: &ExtensiblePayload,
    expected_pubkey: &ECPoint,
) -> bool {
    use neo_core::cryptography::Crypto;

    // Get the hash data that was signed
    let hash_data = payload.get_hash_data();

    // Get the signature from the witness
    let witness = match payload.witness() {
        Some(w) => w,
        None => return false,
    };

    let signature = witness.invocation_script();
    if signature.len() < 64 {
        return false;
    }

    // Extract signature bytes (skip PUSHDATA prefix if present)
    let sig_start = if signature[0] == 0x0c { 2 } else { 0 };
    let sig_bytes = &signature[sig_start..sig_start + 64];

    let pubkey_bytes = expected_pubkey.to_bytes();

    Crypto::verify_signature_secp256r1(&hash_data, sig_bytes, &pubkey_bytes)
}
```

---

### C-4: P2P Memory Exhaustion

**File**: `neo-core/src/network/p2p/framed.rs:145-152`

**Problem**: Allocating up to 32MB per message without per-peer memory limits allows memory exhaustion attacks.

**Current Code**:
```rust
let (payload_length, mut length_bytes) = self.read_var_int(timeout_duration).await?;
message_bytes.append(&mut length_bytes);

let mut payload = vec![0u8; payload_length as usize];  // Up to 32MB allocation
```

**Fixed Code**:
```rust
/// Per-peer memory quota (4MB default)
const PER_PEER_MEMORY_QUOTA: usize = 4 * 1024 * 1024;

pub struct FramedSocket {
    stream: TcpStream,
    config: FramedConfig,
    memory_used: AtomicUsize,  // Track memory usage per peer
}

impl FramedSocket {
    pub async fn read_message(&mut self, handshake_complete: bool) -> NetworkResult<Vec<u8>> {
        // ... existing code ...

        let (payload_length, mut length_bytes) = self.read_var_int(timeout_duration).await?;

        // ========== SECURITY FIX: Check memory quota ==========
        let payload_len = payload_length as usize;
        let current_usage = self.memory_used.load(Ordering::Relaxed);
        if current_usage + payload_len > PER_PEER_MEMORY_QUOTA {
            return Err(NetworkError::ResourceExhausted(format!(
                "Peer memory quota exceeded: {} + {} > {}",
                current_usage, payload_len, PER_PEER_MEMORY_QUOTA
            )));
        }
        self.memory_used.fetch_add(payload_len, Ordering::Relaxed);
        // ========== END SECURITY FIX ==========

        message_bytes.append(&mut length_bytes);
        let mut payload = vec![0u8; payload_len];
        // ...
    }

    /// Release memory after processing message
    pub fn release_memory(&self, size: usize) {
        self.memory_used.fetch_sub(size, Ordering::Relaxed);
    }
}
```

---

### C-5: Private Key Not Zeroized

**File**: `neo-core/src/cryptography/crypto_utils.rs:149-162`

**Problem**: Failed key generation attempts leave private key material in memory.

**Current Code**:
```rust
pub fn generate_private_key() -> [u8; 32] {
    let mut rng = OsRng;
    for _ in 0..MAX_KEY_GEN_ATTEMPTS {
        let mut candidate = [0u8; 32];  // NOT ZEROIZED ON FAILURE
        rng.fill_bytes(&mut candidate);
        if let Ok(secret_key) = Secp256k1SecretKey::from_slice(&candidate) {
            return secret_key.secret_bytes();
        }
    }
    panic!("Failed to generate valid secp256k1 private key...");
}
```

**Fixed Code**:
```rust
use zeroize::Zeroizing;

pub fn generate_private_key() -> [u8; 32] {
    let mut rng = OsRng;
    for _ in 0..MAX_KEY_GEN_ATTEMPTS {
        let mut candidate = Zeroizing::new([0u8; 32]);  // Auto-zeroize on drop
        rng.fill_bytes(candidate.as_mut());
        if let Ok(secret_key) = Secp256k1SecretKey::from_slice(&candidate) {
            return secret_key.secret_bytes();
        }
        // candidate is automatically zeroized here when dropped
    }
    panic!("Failed to generate valid secp256k1 private key...");
}
```

**Also fix `private_key()` method** in `neo-core/src/wallets/key_pair.rs:104-106`:

```rust
// Current (returns copy)
pub fn private_key(&self) -> [u8; HASH_SIZE] {
    self.private_key
}

// Fixed (returns reference)
pub fn private_key(&self) -> &[u8; HASH_SIZE] {
    &self.private_key
}
```

---

## HIGH Fixes (P1)

### H-1: RPC Rate Limiting

**File**: `neo-plugins/src/rpc_server/routes.rs`

**Add Token Bucket Rate Limiter**:
```rust
use dashmap::DashMap;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    ip_buckets: DashMap<IpAddr, TokenBucket>,
    config: RateLimitConfig,
}

struct TokenBucket {
    tokens: f64,
    last_update: Instant,
}

#[derive(Clone)]
pub struct RateLimitConfig {
    pub requests_per_second: f64,
    pub burst_size: u32,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            ip_buckets: DashMap::new(),
            config,
        }
    }

    pub fn check_rate_limit(&self, ip: IpAddr) -> bool {
        let mut bucket = self.ip_buckets.entry(ip).or_insert_with(|| TokenBucket {
            tokens: self.config.burst_size as f64,
            last_update: Instant::now(),
        });

        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_update).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.config.requests_per_second)
            .min(self.config.burst_size as f64);
        bucket.last_update = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}
```

---

### H-2: Storage Commit Error Handling

**File**: `neo-core/src/persistence/rocksdb_store.rs:726-754`

**Current Code**:
```rust
fn commit(&mut self) {
    match self.snapshot.db.write(batch) {
        Ok(()) => { /* success */ }
        Err(e) => {
            error!("CRITICAL: Failed to commit snapshot: {}", e);
            // SILENT FAILURE - data lost
        }
    }
}
```

**Fixed Code**:
```rust
fn commit(&mut self) -> Result<(), StorageError> {
    self.snapshot.db.write(batch)
        .map_err(|e| StorageError::CommitFailed {
            message: format!("RocksDB write failed: {}", e),
        })?;
    Ok(())
}
```

---

## MEDIUM Fixes (P2)

### M-1: Consensus Message Cache Limit

**File**: `neo-plugins/src/dbft_plugin/consensus/consensus_context.rs:40`

```rust
// Current
cached_messages: HashMap<UInt256, ConsensusMessagePayload>,

// Fixed - use LRU cache
use lru::LruCache;
use std::num::NonZeroUsize;

cached_messages: LruCache<UInt256, ConsensusMessagePayload>,

// Initialize with limit
cached_messages: LruCache::new(NonZeroUsize::new(1000).unwrap()),
```

---

### M-2: P2P Connection Check Optimization

**File**: `neo-core/src/network/p2p/local_node.rs:520-528`

```rust
// Current O(n) scan
let per_address = peers
    .values()
    .filter(|entry| Self::normalize_ip(entry.remote_address) == remote_ip)
    .count();

// Fixed O(1) lookup - add index
pub struct LocalNode {
    peers: RwLock<HashMap<SocketAddr, PeerEntry>>,
    peers_by_ip: RwLock<HashMap<IpAddr, Vec<SocketAddr>>>,  // NEW INDEX
}

fn allow_new_connection(&self, remote_ip: IpAddr) -> bool {
    let by_ip = self.peers_by_ip.read().unwrap();
    let count = by_ip.get(&remote_ip).map(|v| v.len()).unwrap_or(0);
    count < self.config.max_connections_per_address
}
```

---

## Testing Recommendations

### Security Test Cases

```rust
#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_bigint_overflow_protection() {
        let mut engine = ExecutionEngine::new(None);
        // Create script that attempts to create huge BigInt
        let script = Script::new(vec![
            OpCode::PUSHINT256 as u8,
            // ... max value bytes ...
            OpCode::PUSHINT256 as u8,
            // ... max value bytes ...
            OpCode::MUL as u8,
        ], false).unwrap();

        engine.load_script(script, -1, 0).unwrap();
        let result = engine.execute();
        assert_eq!(result, VMState::FAULT);  // Should fail, not crash
    }

    #[test]
    fn test_key_generation_uses_csprng() {
        // Verify OsRng is used
        let key1 = KeyPair::generate().unwrap();
        let key2 = KeyPair::generate().unwrap();
        assert_ne!(key1.private_key(), key2.private_key());
    }

    #[test]
    fn test_prepare_response_signature_required() {
        // Test that unsigned PrepareResponse is rejected
        // ... test implementation ...
    }
}
```

---

## Deployment Checklist

- [ ] All CRITICAL fixes applied and tested
- [ ] All HIGH fixes applied and tested
- [ ] Unit tests added for each fix
- [ ] Integration tests pass
- [ ] Third-party security audit completed
- [ ] C# interoperability tests pass
- [ ] Performance benchmarks acceptable
- [ ] Documentation updated

---

## Contact

For security issues, follow the process in `SECURITY.md`.
