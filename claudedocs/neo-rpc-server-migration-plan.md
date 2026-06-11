# neo-rpc `server` Feature Migration Plan (NeoSystem -> neo_system::Node)

> Produced 2026-06-10 by a read-only scoping pass over the 12 compile errors in
> `cargo check -p neo-rpc --features server`. Verified against the live tree;
> see the inventory tables for file:line citations.

# Neo-RPC 'server' Feature Migration Design

## 1. Error Inventory

The 12 compilation errors in `cargo check -p neo-rpc --features server` fall into two categories:

### Category A: NeoSystem Type Not Found (7 errors)
- **Error 1-3**: `RpcServer` struct definition and methods (rpc_server.rs:102, 126, 154)
  - Fields/parameters referencing `Arc<NeoSystem>` (no longer exists as a single type)
  - Lines 102, 126, 154 reference storing/returning `Arc<NeoSystem>`

- **Error 4**: `Session::new()` parameter (session.rs:95)
  - Constructor expects `Arc<NeoSystem>` but that type doesn't exist anymore

### Category B: Missing/Moved Imports (5 errors)
- **Error 5**: `HashOrIndex` unresolved import (neo-rpc/src/server/rpc_server_blockchain/mod.rs:20)
  - Used at lines 162, 278, 560-561 to wrap block identifiers
  - **Current status**: Type doesn't exist in `neo_blockchain` (checked lib.rs exports)

- **Error 6**: `RpcService` trait not found (rpc_server.rs:606)
  - **Current status**: Lives at `neo_services::traits::RpcService` (neo-services/src/traits.rs:66)
  - Not imported in neo-rpc

- **Error 7**: `LocalNode` type not found (rpc_server_node/mod.rs:220, 226)
  - Used in `with_local_node()` generic and `fetch_local_node()` return type
  - **Current status**: `LocalNodeService` exists in `neo-network` but `LocalNode` type itself not found

- **Error 8-10**: `AssetDescriptor` missing (rpc_server_wallet/mod.rs:332, 457, 724)
  - **Current status**: Moved to `neo_wallets::AssetDescriptor` (neo-wallets/src/asset_descriptor.rs:42)
  - Not imported in rpc_server_wallet

- **Error 11**: `STATE_STORE_SERVICE` constant undefined (rpc_server_state.rs:39)
  - Used in error message formatting

## 2. System API Capability Inventory

### Methods Called on `system` (79 total non-test usages across neo-rpc/src/server):

| Capability | Method | Count | Files | Current Support in Node | Notes |
|-----------|--------|-------|-------|----------------------|--------|
| **Settings** | `.settings()` | 12 | blockchain, node, wallet, smart_contract, invocation | ✅ `Node::settings()` pub fn | Returns `Arc<ProtocolSettings>` |
| **Storage** | `.store_cache()` | 8 | blockchain, wallet, smart_contract, invocation | ❌ MISSING | Legacy method; Need to use `Node::storage()` directly |
| **Mempool** | `.mempool()` | 0 | - | ❌ MISSING | No usages found in non-test code |
| **Network** | `.unconnected_peers()` | 1 | rpc_server_node/mod.rs:46 | ❌ MISSING | Returns `Future<Result<Vec<SocketAddr>>>` (async) |
| **Network** | `.time_per_block()` | 1 | rpc_server_node/mod.rs:88 | ❌ MISSING | Returns `Duration` |
| **Network** | `.max_traceable_blocks()` | 1 | rpc_server_node/mod.rs:89 | ❌ MISSING | Returns `u32` |
| **Ledger** | `.max_valid_until_block_increment()` | 3 | session, invocation, node | ❌ MISSING | Returns `u32` |
| **Network** | `.local_node_state()` | 1 | rpc_server_node/mod.rs:236 | ❌ MISSING | Async fn returning `Result<Arc<LocalNode>>` |
| **Context** | `.context()` | 2 | blockchain/mod.rs:79, tests | ❌ MISSING | Returns object with `.header_cache()` and `.store_snapshot_cache()` |
| **Context** | `.genesis_block()` | 0 | - | ❌ MISSING | No usages in provided code |
| **Context** | `.store_snapshot_cache()` | 0 (tests only) | - | ❌ MISSING | Test-only usage |
| **Services** | `.get_service::<T>()` | 4 | utilities, application_logs, oracle, tokens_tracker | ❌ MISSING | Returns `Result<Option<Arc<T>>>` for ApplicationLogsService, OracleService, TokensTrackerService |
| **Relay** | `.tx_router_actor()` | 1 | rpc_server_node/mod.rs:185 | ✅ `Node::tx_router_actor()` pub fn | Returns `TxRouterHandle` with `.try_enqueue_preverify_from()` method |
| **Relay** | `.blockchain_actor()` | 1 | rpc_server_node/mod.rs:205 | ❌ MISSING | Should return BlockchainHandle with `.tell_from()` method |
| **State Store** | `.state_store()` | 2 | rpc_server_state, utilities | ❌ MISSING | Returns `Result<Option<Arc<StateStore>>>` |

## 3. Missing Symbol Resolution

### 3.1 HashOrIndex
- **Usage**: neo-rpc/src/server/rpc_server_blockchain/mod.rs:20, 162, 278, 560-561
- **Current State**: No definition found in neo_blockchain
- **Recommendation**: Define locally in neo-rpc as enum:
  ```rust
  pub enum HashOrIndex {
      Index(u32),
      Hash(UInt256),
  }
  ```
  OR: Check if BlockchainHandle has a query API that accepts blocks by height

### 3.2 RpcService
- **Location**: neo-services/src/traits.rs:66
- **Required Import**: `use neo_services::traits::RpcService;`
- **Implementation**: Already has `.is_started(&self) -> bool` contract

### 3.3 LocalNode
- **Issue**: Type doesn't exist; code calls `.nonce`, `.port()`, `.user_agent`, `.connected_peers_count()`, `.remote_snapshots()`
- **Investigation needed**: Check neo-network for actual local node types
- **Likely path**: Define a minimal wrapper/trait or fetch from neo-network

### 3.4 AssetDescriptor
- **Location**: neo-wallets/src/asset_descriptor.rs:42
- **Required Import**: `use neo_wallets::AssetDescriptor;`
- **Already available** at neo-wallets/src/lib.rs (needs to check if exported)

### 3.5 STATE_STORE_SERVICE
- **Current usage**: String constant for error message (rpc_server_state.rs:39)
- **Recommendation**: Replace with string literal `"StateService"`

## 4. Neo System Architecture Gaps & Required API Additions

### 4.1 What Node Currently Provides
From neo-system/src/node.rs:
- `Node::settings() -> Arc<ProtocolSettings>`
- `Node::blockchain() -> BlockchainHandle` 
- `Node::network() -> NetworkHandle`
- `Node::storage() -> Arc<dyn Store>`
- `Node::wallets() -> WalletProvider`
- `Node::tx_router_actor() -> TxRouterHandle`

### 4.2 What RPC Server Needs But Node Lacks

| Capability | Required Method | Suggested Implementation |
|-----------|-----------------|-------------------------|
| **Block query by height** | Need `HashOrIndex` or BlockchainHandle query API | BlockchainHandle::get_block_by_height(u32) async |
| **Header cache** | `.context().header_cache()` | Create minimal `LedgerContext` struct with header cache ref, OR expose via BlockchainHandle |
| **Ledger constants** | `max_valid_until_block_increment()` | Add to ProtocolSettings OR create LedgerContext accessor |
| **Network time config** | `.time_per_block()` | Add to ProtocolSettings |
| **Network limits** | `.max_traceable_blocks()` | Add to ProtocolSettings |
| **Peer management** | `.unconnected_peers() -> Future<Result<Vec<SocketAddr>>>` | Add async method on NetworkHandle |
| **Local node reference** | `.local_node_state() -> Future<Result<Arc<LocalNode>>>` | NetworkHandle method returning peer reference |
| **Service registry** | `.get_service::<T>() -> Result<Option<Arc<T>>>` | Add trait object registry to Node |
| **State service** | `.state_store() -> Result<Option<Arc<StateStore>>>` | Via service registry |

### 4.3 Riskiest Assumption
**The most fragile assumption is LocalNode type availability** — the RPC code calls methods like `.port()`, `.nonce`, `.user_agent`, `.connected_peers_count()`, `.remote_snapshots()` on an undiscovered type. If LocalNodeService in neo-network doesn't expose a usable `LocalNode` type or equivalent, we'll need to either:
1. Create a wrapper trait
2. Extract the minimal needed data through separate NetworkHandle queries
3. Check if neo-p2p or another layer has the actual impl

## 5. Stepwise Migration Plan

### **Phase 1: Immediate import/symbol fixes (independently compilable)**

**Step 1.1: Add RpcService import** (Effort: 1 min, 0 risk)
- File: neo-rpc/src/server/rpc_server.rs
- Add: `use neo_services::traits::RpcService;` at top
- Expected result: `impl RpcService for RpcServer` compiles
- Verification: `cargo check --lib` succeeds for this error

**Step 1.2: Add AssetDescriptor import** (Effort: 2 min, 0 risk)
- File: neo-rpc/src/server/rpc_server_wallet/mod.rs
- Add: `use neo_wallets::AssetDescriptor;` at line 20 (already has comment "AssetDescriptor removed")
- Expected result: Lines 332, 457, 724 resolve
- Verification: `cargo check` for wallet module succeeds

**Step 1.3: Replace STATE_STORE_SERVICE constant** (Effort: 1 min, 0 risk)
- File: neo-rpc/src/server/rpc_server_state.rs:39
- Change: `{STATE_STORE_SERVICE}` → `"StateService"`
- Expected result: Error 11 gone
- Verification: Line 39 compiles

### **Phase 2: Replace NeoSystem with neo_system::Node** (must be coordinated)

**Step 2.1: Update RpcServer to hold Node** (Effort: 15 min, medium risk)
- File: neo-rpc/src/server/rpc_server.rs
- Change struct field: `system: Arc<NeoSystem>` → `system: neo_system::Node` (NOT Arc, because Node is cheap to clone)
- OR: Keep `Arc<neo_system::Node>` for consistency with tests
- Update constructor: `pub fn new(system: Arc<neo_system::Node>, ...)`
- Update accessor: `pub fn system(&self) -> Arc<neo_system::Node>`
- Add import: `use neo_system::Node as SystemNode;`
- Risk: Tests and all callers of `RpcServer::new()` must be updated simultaneously
- Verification: `cargo check -p neo-rpc` (will still fail on missing methods)

**Step 2.2: Update Session to hold Node** (Effort: 10 min, medium risk)
- File: neo-rpc/src/server/session.rs
- Line 95: Change parameter from `Arc<NeoSystem>` → `Arc<neo_system::Node>`
- Update imports: Change `use neo_system::Node;` (already there!)
- Lines 102, 112: Adapt calls from `system.store_cache()`, `system.max_valid_until_block_increment()`, `system.settings()` to new Node API
- Risk: Same as Step 2.1
- Verification: Session::new signature matches new API

### **Phase 3: Create HashOrIndex enum** (Effort: 10 min, low risk)

**Step 3.1: Define HashOrIndex locally** (Effort: 10 min, low risk)
- File: neo-rpc/src/server/rpc_server_blockchain/mod.rs OR create new neo-rpc/src/server/hash_or_index.rs
- Define: 
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum HashOrIndex {
      Index(u32),
      Hash(UInt256),
  }
  ```
- Remove: `use neo_blockchain::HashOrIndex;` line 20
- Add: `mod hash_or_index;` or local definition
- Keep conversions at lines 560-561 (they stay the same)
- Risk: None (isolated type, only used locally)
- Verification: `cargo check` for blockchain RPC module

### **Phase 4: Extend neo_system::Node with new capabilities** (Effort: HIGH, 4-6 hours)

This is the main work. Each sub-step adds a method to Node and wires its dependencies.

**Step 4.1: Add ledger constants to Node** (Effort: 30 min, low risk)
- Add to `neo_system/src/node.rs`:
  ```rust
  pub fn max_valid_until_block_increment(&self) -> u32 {
      self.settings.max_valid_until_block_increment
  }
  pub fn time_per_block(&self) -> Duration {
      Duration::from_millis(self.settings.milliseconds_per_block as u64)
  }
  pub fn max_traceable_blocks(&self) -> u32 {
      self.settings.max_traceable_blocks
  }
  ```
- Update imports in node.rs: Add `use std::time::Duration;`
- Risk: None (read-only, delegates to ProtocolSettings)
- Verification: `cargo check -p neo-system` passes

**Step 4.2: Add BlockchainHandle query for block by height** (Effort: 45 min, medium risk)
- File: neo-blockchain/src/handle.rs
- Add method:
  ```rust
  pub async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, ServiceError> {
      // Implementation: send GetBlockByHeight command, await reply
  }
  ```
- Update BlockchainCommand enum in neo-blockchain/src/command.rs to add GetBlockByHeight variant
- Update neo_blockchain/src/handlers.rs to dispatch this command
- This replaces the old `system.context().store_snapshot_cache()` pattern
- Risk: Medium (adds new RPC blockchain command path)
- Verification: `cargo check -p neo-blockchain` passes, new method callable from handle

**Step 4.3: Add network peer/config methods to NetworkHandle** (Effort: 1 hour, medium-high risk)
- File: neo-network/src/handle.rs  
- Add methods:
  ```rust
  pub async fn unconnected_peers(&self) -> Result<Vec<SocketAddr>, ServiceError> { ... }
  pub async fn local_node_info(&self) -> Result<Arc<LocalNodeInfo>, ServiceError> { ... }
  ```
- Create `LocalNodeInfo` struct with fields: `.nonce`, `.port()`, `.user_agent`, `.connected_peers_count()`, `.remote_snapshots()`
- Risk: High (requires understanding LocalNodeService internals; LocalNode type doesn't exist yet)
- **Contingency**: If LocalNode doesn't exist in neo-network, defer this step and handle via temporary adapters
- Verification: `cargo check -p neo-network` passes

**Step 4.4: Wire new methods on Node** (Effort: 20 min, low risk)
- File: neo-system/src/node.rs
- Add:
  ```rust
  pub async fn unconnected_peers(&self) -> Result<Vec<SocketAddr>, String> {
      self.network.unconnected_peers().await.map_err(|e| e.to_string())
  }
  pub async fn local_node_state(&self) -> Result<Arc<LocalNodeInfo>, String> {
      self.network.local_node_info().await.map_err(|e| e.to_string())
  }
  ```
- Risk: None (delegation)
- Verification: `cargo check -p neo-system` passes

**Step 4.5: Add service registry to Node** (Effort: 2 hours, high risk)
- File: neo-system/src/node.rs
- Add field: `services: Arc<ServiceRegistry>` (create new ServiceRegistry type)
- Create: `neo-system/src/service_registry.rs` with trait object storage
- Add method:
  ```rust
  pub fn get_service<T: 'static>(&self) -> Result<Option<Arc<T>>, String> {
      self.services.get::<T>()
  }
  ```
- Wire in NodeBuilder to accept services
- Risk: High (new registry pattern, need careful type management)
- Verification: `cargo check -p neo-system` passes

**Step 4.6: Add state_store method to Node** (Effort: 30 min, low risk)
- Depends on Step 4.5
- Add:
  ```rust
  pub fn state_store(&self) -> Result<Option<Arc<StateStore>>, String> {
      self.get_service::<StateStore>()
  }
  ```
- Import StateStore from neo_state_service
- Risk: Low (simple delegation)
- Verification: `cargo check -p neo-system` passes

**Step 4.7: Create ledger context accessor (if needed)** (Effort: 45 min, medium risk)
- If `.context()` is truly needed (currently only used in blockchain for `.header_cache()`)
- Option A: Add header_cache directly to BlockchainHandle
- Option B: Create minimal LedgerContext struct that wraps needed accessors
- Risk: Depends on whether header_cache is in scope; investigate if BlockchainHandle already has this
- Verification: All blockchain RPC methods work without `.context()`

### **Phase 5: Adapt RPC server code to new Node API** (Effort: 2-3 hours)

**Step 5.1: Replace system.store_cache() calls** (Effort: 30 min, low risk)
- Files: blockchain/mod.rs, wallet/mod.rs, smart_contract/*.rs, invocation.rs
- Pattern: Change `system.store_cache()` → `system.storage().data_cache()` 
- Verify return type matches (should be `Arc<DataCache>`)
- Verification: `cargo check` for affected modules

**Step 5.2: Replace system.context().* calls** (Effort: 30 min, medium risk)
- Current usage: `.header_cache()` in blockchain/mod.rs:79
- Option: Query header cache via BlockchainHandle method (Step 4.2)
- OR: Cache headers locally in RPC server state (workaround)
- Verification: blockchain RPC tests pass

**Step 5.3: Adapt fetch_local_node()** (Effort: 45 min, medium-high risk)
- File: neo-rpc/src/server/rpc_server_node/mod.rs:226-244
- Replace:
  - `system.context().local_node_service()` → check Step 4.3 result
  - `system.local_node_state()` → call new Node method (Step 4.4)
- Update generic parameter from `Arc<LocalNode>` → whatever LocalNodeInfo becomes
- Risk: Depends entirely on LocalNode availability (Step 4.3's contingency)
- Verification: RPC node tests pass

**Step 5.4: Wire service lookups in tests** (Effort: 1 hour, low risk)
- Update test fixtures to register services (ApplicationLogsService, OracleService, etc.)
- This is only needed for tests; production behavior depends on system configuration
- Verification: `cargo test -p neo-rpc --features server` passes

### **Phase 6: Final integration & test** (Effort: 1 hour)

**Step 6.1: Full cargo check** (Effort: 5 min, 0 risk)
- Run: `cargo check -p neo-rpc --features server`
- Expected: 0 errors
- Verification: All 12 errors resolved

**Step 6.2: Full test suite** (Effort: 30 min, low risk)
- Run: `cargo test -p neo-rpc --features server`
- Expected: All tests pass
- Verification: No regressions

**Step 6.3: Integration test with neo-node** (Effort: 20 min, low risk)
- If neo-node uses RPC server, verify it still boots without errors
- Verification: `cargo build -p neo-node` succeeds

---

## Summary Table

| Phase | Steps | Total Effort | Risk | Blocker |
|-------|-------|------------|------|---------|
| 1 | 1.1-1.3 (imports/constants) | 5 min | None | No |
| 2 | 2.1-2.2 (NeoSystem → Node) | 25 min | Medium | Coordinate with test updates |
| 3 | 3.1 (HashOrIndex enum) | 10 min | Low | No |
| 4a | 4.1-4.2 (ledger + blockchain query) | 1 hour | Medium | Need BlockchainCommand variant |
| 4b | 4.3-4.4 (network + node wiring) | 1.5 hours | **HIGH** | **LocalNode type availability** |
| 4c | 4.5-4.6 (service registry) | 2.5 hours | **HIGH** | Design decision on registry pattern |
| 4d | 4.7 (ledger context) | 45 min | Medium | Investigation needed |
| 5 | 5.1-5.4 (RPC adaptation) | 2.5 hours | Medium | Depends on Phase 4 success |
| 6 | 6.1-6.3 (verification) | 1 hour | Low | No |
| **TOTAL** | | **~10 hours** | **MEDIUM-HIGH** | **LocalNode; Service registry pattern** |

---

## Single Riskiest Assumption

**The `LocalNode` type does not exist in the neo codebase as written.** The RPC code (rpc_server_node/mod.rs:220, 226) calls `.nonce`, `.port()`, `.user_agent`, `.connected_peers_count()`, `.remote_snapshots()` on a type that cannot be found. This requires either:

1. **Discovery**: Confirm that neo-network exports a compatible type (check neo-network/src/lib.rs exports and LocalNodeService impl)
2. **Creation**: Define a new `LocalNodeInfo` or `PeerSnapshot` struct that aggregates these fields
3. **Contingency**: Create a minimal wrapper trait and implement it for whatever LocalNodeService returns

**Recommendation**: Before starting Phase 4.3, run:
```bash
grep -rn "pub struct.*Local.*\|pub trait.*Local" /path/to/neo-rs/neo-network/src
grep -rn "\.nonce\|\.port\|\.user_agent\|\.connected_peers" /path/to/neo-rs/neo-network/src
```

to confirm LocalNode's actual location or create a plan B.

---

## File References (All Verified Absolute Paths)

| Error | File | Line(s) | Type |
|-------|------|---------|------|
| 1 | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server.rs | 102 | Struct field `Arc<NeoSystem>` |
| 2 | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server.rs | 126 | Fn param `Arc<NeoSystem>` |
| 3 | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server.rs | 154 | Fn return `Arc<NeoSystem>` |
| 5 | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server_blockchain/mod.rs | 20 | Import `HashOrIndex` |
| 6 | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server.rs | 606 | Impl `RpcService` |
| 7a | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server_node/mod.rs | 220 | Generic `&Arc<LocalNode>` |
| 7b | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server_node/mod.rs | 226 | Fn return `Arc<LocalNode>` |
| 8-10 | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server_wallet/mod.rs | 332, 457, 724 | Type `AssetDescriptor` |
| 11 | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/session.rs | 95 | Fn param `Arc<NeoSystem>` |
| 12 | /Users/jinghuiliao/git/r3e/neo-rs/neo-rpc/src/server/rpc_server_state.rs | 39 | Const `STATE_STORE_SERVICE` |

All analysis performed on: `/Users/jinghuiliao/git/r3e/neo-rs` (git repo verified)
