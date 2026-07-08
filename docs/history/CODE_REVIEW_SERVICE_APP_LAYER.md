# Code Review: Node Service Layer & Application Layer

**Project**: neo-rs (Neo N3 v3.10.1)
**Date**: 2026-07-03  
**Scope**: 8 crates — node service layer + application layer  
**Review type**: READ-ONLY architecture, protocol, error handling, performance, security

---

## 1. Service Architecture — Reth-Style Command Channels

### 1.1 [HIGH] Unbounded batched drain starves the async runtime

**File**: `neo-blockchain/src/service/service.rs:168-178`

```rust
while let Some(cmd) = self.cmd_rx.recv().await {
    self.dispatch(cmd).await;
    // Drain all remaining pending commands without yielding to the
    // runtime — keeps the pipeline full during catch-up bursts.
    while let Ok(cmd) = self.cmd_rx.try_recv() {
        self.dispatch(cmd).await;
    }
}
```

The inner `try_recv` loop is unbounded. During sustained block bursts (500+ blocks), the loop can run indefinitely without yielding to the tokio runtime. Other async tasks on the same runtime — the network service, event subscribers, RPC handlers — will be starved until the drain completes.

**Recommendation**: Add a drain cap (e.g., process at most 1,024 commands per drain cycle, then `tokio::task::yield_now().await`). This preserves throughput while keeping other tasks responsive.

### 1.2 [MEDIUM] BlockchainService has no graceful shutdown path

**File**: `neo-blockchain/src/service/service.rs:168-179`  
**File**: `neo-blockchain/src/service/handle.rs:294-302`

`BlockchainHandle::shutdown()` merely drops a single clone of the sender channel. The service loop terminates only when **all** sender clones are dropped. Unlike the network service, which has `NetworkCommand::Shutdown` + `CancellationToken`-based cleanup, here no explicit shutdown sequence exists.

```rust
pub async fn shutdown(&self) -> Result<(), ServiceError> {
    // The service loop is driven by `recv().await`; closing the
    // sender is the canonical shutdown signal. We don't expose a
    // dedicated `Shutdown` variant yet because the legacy command
    // set never used one — the service stops on its own once all
    // senders are dropped.
    drop(self.cmd_tx.clone());
    Ok(())
}
```

The known limitation is explicit in comments, but it weakens node shutdown determinism. If any component retains a handle clone (RPC server, consensus driver, plugins), the service never drops.

**Recommendation**: Add `BlockchainCommand::Shutdown` that flushes the mempool, commits the store, and breaks the run loop. Follow the `NetworkCommand::Shutdown` + `CancellationToken` pattern.

### 1.3 [LOW] NetworkHandle::try_broadcast_transaction conflates full-channel with shutdown

**File**: `neo-network/src/service/handle.rs:424-435`

```rust
pub fn try_broadcast_transaction(&self, transaction: Transaction) -> NetworkResult<()> {
    use tokio::sync::mpsc::error::TrySendError;
    match self.cmd_tx.try_send(NetworkCommand::BroadcastTransaction { transaction }) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_) | TrySendError::Closed(_)) => {
            Err(NetworkError::LocalShuttingDown)
        }
    }
}
```

Both `TrySendError::Full` and `TrySendError::Closed` return the same error (`LocalShuttingDown`), making it impossible for callers to distinguish backpressure from actual shutdown. A saturated send buffer (e.g., during high-volume gossip) is misreported as a fatal shutdown.

**Recommendation**: Return distinct errors for `Full` vs `Closed`, or at minimum add a `NetworkError::ChannelFull` variant so callers can apply backoff rather than treating it as terminal.

---

## 2. Protocol Compatibility — Network & RPC

### 2.1 [MEDIUM] StackValue::Pointer truncation diverges from C# int semantics

**File**: `neo-blockchain/src/pipeline/native_persist.rs:838-839`

```rust
StackItem::Pointer(pointer) => {
    StackValue::Pointer(i64::try_from(pointer.position()).unwrap_or(i64::MAX))
}
```

C# `VM.Types.Pointer.Position` is an `int` (32-bit signed). The Rust version uses `usize` → `i64` conversion with silent `i64::MAX` fallback on overflow. On 64-bit platforms, theoretically reachable (though practically unlikely with normal stack sizes). If a VM implementation somehow creates a pointer at position > `i64::MAX`, the C# node would throw `OverflowException` while this code silently mutates the value.

**Recommendation**: Assert or saturate the `usize` → `i32` range with explicit error logging, matching C# behavior exactly.

### 2.2 [MEDIUM] CORS origin handling: case-insensitive match, case-preserving echo

**File**: `neo-rpc/src/server/rpc_server/http_policy.rs:54-65`

```rust
} else if self.allow_origins.iter().any(|origin| origin.eq_ignore_ascii_case(requested_origin)) {
    requested_origin.to_string()
}
```

Allowed origins are verified with case-insensitive comparison but the response echoes the **client's** casing. While not a security vulnerability per se (the origin header is set by the browser, not the server), this deviates from the CORS specification's recommendation for exact matching. Some strict CORS middleware on the client side may reject responses where `Access-Control-Allow-Origin` does not match the allowed origin exactly.

**Recommendation**: Echo the **canonical** allowed origin (the one from the config), not the client-sent value.

### 2.3 [LOW] system_fee summation overflow on large blocks

**File**: `neo-rpc/src/server/rpc_server_blockchain/mod.rs:207-211`

```rust
let system_fee: i64 = block.transactions.iter()
    .map(neo_payloads::Transaction::system_fee)
    .sum();
```

`system_fee` returns `i64`. In theory, on blocks with thousands of high-fee transactions, `sum()` could overflow. C# also uses `long` sums without overflow checks, so parity is maintained — but this could produce incorrect RPC responses.

**Recommendation**: Use `saturating_add` or checked addition, logging a warning on overflow.

---

## 3. Error Handling — Panic, Unwrap, Expect

### 3.1 [MEDIUM] `block_on_service` creates throwaway tokio Runtime when not in async context

**File**: `neo-rpc/src/server/rpc_relay/mod.rs:88-98`

```rust
pub(super) fn block_on_service<F, T>(future: F) -> Result<T, RpcException>
where F: std::future::Future<Output = T>,
{
    if let Ok(handle) = Handle::try_current() {
        Ok(block_in_place(|| handle.block_on(future)))
    } else {
        let runtime = Runtime::new().map_err(...)?;
        Ok(runtime.block_on(future))
    }
}
```

Creating a throwaway `Runtime` allocates a full multi-threaded tokio runtime on every fallback call. The comment says this path is "for direct handler invocation in tests", but the branching logic is live in production code. If something causes `Handle::try_current()` to return `Err` during normal RPC handling, the performance impact would be severe.

**Recommendation**: Inject the runtime handle at construction time or use `tokio::runtime::Builder::new_current_thread()` for the fallback path.

### 3.2 [MEDIUM] Silent error discard on `BlockchainCommand::Import` without reply

**File**: `neo-blockchain/src/service/service.rs:188-190`

```rust
BlockchainCommand::Import { import } => {
    let _ = self.handle_import(import).await;
}
```

When callers use `tell(Command::Import { ... })` instead of the proper `import_blocks()` API (which sends `ImportBlocks { import, reply }`), all import errors are silently discarded. Callers have no indication whether blocks were accepted, rejected, or partially processed.

**Recommendation**: Remove the `Import` variant or convert callers to always use the reply-acknowledged path `ImportBlocks`. If fire-and-forget is truly needed, at minimum log the error at `warn` level instead of using `let _`.

### 3.3 [LOW] usize → i64 conversions silently saturate to i64::MAX

**File**: `neo-node/src/node/telemetry/exporter.rs:403-405`

```rust
fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
```

Used in metrics rendering. On 64-bit platforms with legitimate metric values exceeding i64::MAX (5+ billion peers or 5+ billion mempool transactions — practically unreachable), the value silently caps. While effectively unreachable in practice, the `i64::MAX` value could confuse monitoring dashboards by looking like a valid-but-unusual metric.

**Recommendation**: At minimum cast `as i64` or saturate to `-1` to indicate "overflow" rather than a plausible large number.

### 3.4 [LOW] Telemetry RocksDB metrics silently return empty on non-RocksDB backends

**File**: `neo-node/src/node/telemetry/exporter.rs:189-196`

```rust
let Some(store) = storage.as_any().downcast_ref::<neo_storage::rocksdb::RocksDbStore>()
else { return String::new(); };
```

When the storage backend is not RocksDB (e.g., in-memory store for tests, or a future alternative backend), the RocksDB metrics panel renders as empty without any log or indicator. Operators running alternative backends get no signal that the metrics panel is inapplicable.

**Recommendation**: Emit a `neo_storage_backend{type="memory|rocksdb|..."}` gauge so operators can correlate missing metrics to backend type.

---

## 4. Performance — Blocking in Async, Unnecessary Allocations

### 4.1 [MEDIUM] Deep StackItem cloning in notification collection

**File**: `neo-blockchain/src/pipeline/native_persist.rs:846-856`

```rust
fn collect_notifications(engine: &ApplicationEngine) -> Vec<NativePersistNotification> {
    engine.notifications().iter().map(|event| NativePersistNotification {
        script_hash: event.script_hash,
        event_name: event.event_name.clone(),
        state: event.state.clone(),  // <-- deep clone of Vec<StackItem>
    }).collect()
}
```

`StackItem::Array`, `StackItem::Struct`, and `StackItem::Map` variants contain nested child items. A NEP-17 Transfer notification carries 4 items (from_hash, to_hash, amount, token_hash), which is cheap. However, custom smart contracts can emit large nested notification states (e.g., a Map with hundreds of entries), and these are cloned in full each time.

**Recommendation**: Store notifications as `Arc<Vec<StackItem>>` or use the engine's existing references if notification lifetimes allow borrowing.

### 4.2 [LOW] LedgerContract::new() allocated per RPC call

**File**: `neo-rpc/src/server/rpc_server_blockchain/mod.rs:70,84,124,146,170,194,254,294,355,469,481`

Every blockchain RPC handler creates its own `LedgerContract::new()`. This is a lightweight struct but C# uses a static singleton. While the Rust cost is negligible (no heap allocation), the pattern is repeated across ~15 handlers in one file.

**Recommendation**: Not a performance issue in practice, but centralizing to a `lazy_static`/`LazyLock` or storing it on `RpcServer` would reduce code verbosity and follow C# singleton semantics.

### 4.3 [LOW] Unnecessary Arc::clone chains in RPC get_next_block_validators/get_committee/get_candidates

**File**: `neo-rpc/src/server/rpc_server_blockchain/mod.rs:393,397,404,430,434,441,482,484`

```rust
let snapshot = std::sync::Arc::new(store.data_cache().clone());
// Then repeatedly:
std::sync::Arc::clone(&snapshot)
```

`store.data_cache().clone()` performs a shallow copy of the DataCache handle (ref-counted internally), so this is cheap. But the pattern is verbose and repeated.

**Recommendation**: Extract the snapshot once and pass by reference, or accept `&DataCache` in the helper functions instead of `Arc<DataCache>`.

---

## 5. Security — Unsafe Code, DoS Vectors

### 5.1 [INFO] Oracle SSRF protection is well-implemented

**File**: `neo-oracle-service/src/https/security.rs:27-35,102-171`

The SSRF filter correctly:
- Checks ALL DNS-resolved IPs, preventing multi-A-record rebinding attacks
- Blocks IPv4-mapped IPv6 addresses preventing bypasses
- Rejects URL-encoded and octal-notation IP addresses
- Restricts to `http`/`https` schemes only
- Blocks credentials in URLs
- Blocks common internal service ports

This is a strong implementation. No issues found.

### 5.2 [INFO] RPC Session uses Mutex over RwLock to guard unsafe pointer

**File**: `neo-rpc/src/server/rpc_server/mod.rs:128-132`

```rust
/// Sessions contain `ApplicationEngine` which wraps `ExecutionEngine` with a raw pointer
/// that is NOT thread-safe. Using `Mutex` instead of `RwLock` prevents accidental
/// concurrent reads that would cause undefined behavior.
sessions: Arc<Mutex<HashMap<Uuid, Session>>>,
```

The design notes that `ExecutionEngine` contains a raw pointer and correctly uses `Mutex` (exclusive access) rather than `RwLock` to prevent data races. This is correct.

**Recommendation**: Add `// SAFETY:` comments to the `ExecutionEngine` raw pointer declaration explaining the invariant — that the pointer is only accessed under `Mutex` guard and never escapes the locked scope.

### 5.3 [LOW] Prometheus metrics counter registration races on duplicate names

**File**: `neo-rpc/src/server/rpc_server/mod.rs:97-103,109-116`

```rust
let counter = Counter::new("neo_rpc_requests_total", "Total RPC requests")
    .unwrap_or_else(|_| Counter::new("neo_rpc_requests_total_invalid", "Invalid")
        .expect("fallback counter creation should never fail"));
if let Err(err) = prometheus::register(Box::new(counter.clone())) {
    warn!("Failed to register neo_rpc_requests_total: {}", err);
}
```

When the counter name is already registered (e.g., from a previous `register()` call), the fallback creates a differently-named counter. This means some instances will report metrics under `neo_rpc_requests_total` while others (after a registration conflict) will report under `neo_rpc_requests_total_invalid`. Beyond the small behavioral risk, `register()` returns an error for already-registered metrics, which the code logs but silently continues — the counter won't expose any value.

**Recommendation**: Use `prometheus::register!(counter.clone())` macro which handles deduplication, or check `register()` return and use the already-registered counter from the registry.

### 5.4 [LOW] No payload size enforcement on RPC params

The RPC handlers pass request parameters directly to query functions without size bounds. For example, `getblock` or `getblockheader` with `verbose=true` fetches and serializes a full block. While the Neo protocol limits block size (MAX_BLOCK_SIZE), a future misconfiguration or unexpectedly large block could cause memory spikes. This is a minor concern given protocol-level constraints.

**Recommendation**: Consider adding a maximum response size guard or body-size limiter at the transport level if not already present in the jsonrpsee middleware.

---

## 6. Additional Observations

### 6.1 Positive Patterns

The following architectural decisions are notably solid:

- **No `unsafe` blocks** in the network or RPC service layers. All unsafe code is contained within `neo-vm`/`neo-execution` where VM semantics require it.
- **Reth-style channels** (`mpsc` + `oneshot` replies) cleanly replace the legacy actor pattern across both blockchain and network services.
- **`CancellationToken`**-based shutdown is correctly propagated through the network service accept loop and per-peer tasks.
- **Handle-side peer tracking** (`LocalNodeState` / `PeerTracker`) avoids round-trips to the service for `getpeers` RPC queries.
- **SSRF protection** in the Oracle service is thorough and matches C# defense-in-depth.
- **Per-block atomic child caches** in native persistence correctly prevent partial state from becoming visible — this is critical for C# parity.

### 6.2 Summary

| Severity | Count | Key Areas |
|----------|-------|-----------|
| HIGH     | 1     | Unbounded batch drain starving async runtime |
| MEDIUM   | 6     | Shutdown determinism, error discards, pointer truncation, throwaway Runtime, CORS alignment, StackItem cloning |
| LOW      | 6     | Error conflation, silent metric fallbacks, overflow encoding, unnecessary allocations |
| INFO     | 3     | SSRF quality, Session safety, Counter race |
