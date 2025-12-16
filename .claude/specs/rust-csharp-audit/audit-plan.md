# Neo-rs vs Neo C# Full Audit Plan

**Version**: 1.0.0
**Date**: 2025-12-16
**Scope**: All Rust crates vs corresponding C# modules

---

## Module Mapping

| Rust Crate | C# Module | Priority | Complexity |
|------------|-----------|----------|------------|
| `neo-vm` | `Neo.VM` | P0 | High |
| `neo-crypto` | `Neo.Cryptography.BLS12_381`, `Neo.Cryptography.MPTTrie` | P0 | High |
| `neo-core` | `Neo` (main module) | P0 | Very High |
| `neo-consensus` | `Plugins/DBFTPlugin` | P0 | High |
| `neo-json` | `Neo.Json` | P1 | Low |
| `neo-io` | `Neo.IO` | P1 | Medium |
| `neo-primitives` | `Neo` (types) | P1 | Medium |
| `neo-rpc` | `RpcClient`, `Plugins/RpcServer` | P1 | High |
| `neo-storage` | `Plugins/LevelDBStore` | P2 | Medium |
| `neo-state` | `Plugins/StateService` | P2 | High |
| `neo-p2p` | `Neo/Network/P2P` | P2 | High |
| `neo-mempool` | `Neo/Ledger/MemoryPool` | P2 | Medium |
| `neo-cli` | `Neo.CLI` | P3 | Low |
| `neo-config` | `Neo/ProtocolSettings` | P3 | Low |
| `neo-node` | `Neo.CLI` (runtime) | P3 | Medium |

---

## Audit Tasks

### Task 1: neo-vm vs Neo.VM
**Files to Compare**:
- Rust: `neo-vm/src/` (execution_engine, script_builder, opcodes, stack_item)
- C#: `neo_csharp/src/Neo.VM/` (ExecutionEngine.cs, ScriptBuilder.cs, OpCode.cs, Types/)

**Audit Checklist**:
- [ ] OpCode enum completeness (all 200+ opcodes)
- [ ] ExecutionEngine state machine logic
- [ ] Stack operations (push, pop, peek, swap, roll)
- [ ] JumpTable instruction handlers
- [ ] StackItem types (Integer, ByteString, Array, Map, Struct, Boolean, Null, Pointer)
- [ ] Script parsing and validation
- [ ] Gas metering logic
- [ ] Exception handling (TRY/CATCH/FINALLY)

---

### Task 2: neo-crypto vs Neo.Cryptography
**Files to Compare**:
- Rust: `neo-crypto/src/` (hash, ecc, bls12_381, mpt_trie)
- C#: `neo_csharp/src/Neo.Cryptography.BLS12_381/`, `Neo.Cryptography.MPTTrie/`

**Audit Checklist**:
- [ ] SHA256, RIPEMD160, Hash160, Hash256
- [ ] secp256r1/secp256k1 ECDSA signatures
- [ ] BLS12-381 curve operations (G1, G2, Fp, Fp2, Fp6, Fp12, pairing)
- [ ] MPT Trie (insert, delete, get, proof generation/verification)
- [ ] Murmur3 hash
- [ ] Base58/Base64 encoding

---

### Task 3: neo-consensus vs DBFTPlugin
**Files to Compare**:
- Rust: `neo-consensus/src/` (service.rs, context.rs, messages/)
- C#: `neo_csharp/src/Plugins/DBFTPlugin/` (ConsensusService.cs, ConsensusContext.cs, Messages/)

**Audit Checklist**:
- [ ] dBFT 2.0 state machine (Initial, Primary, Backup, ViewChanging, Committed)
- [ ] Message types (PrepareRequest, PrepareResponse, Commit, ChangeView, RecoveryMessage)
- [ ] Signature verification flow
- [ ] View change logic
- [ ] Recovery message handling
- [ ] Timer/timeout handling
- [ ] Block proposal creation

---

### Task 4: neo-core vs Neo (main)
**Files to Compare**:
- Rust: `neo-core/src/` (smart_contract/, ledger/, network/, persistence/)
- C#: `neo_csharp/src/Neo/` (SmartContract/, Ledger/, Network/, Persistence/)

**Audit Checklist**:
- [ ] Native contracts (NeoToken, GasToken, PolicyContract, ContractManagement, RoleManagement)
- [ ] ApplicationEngine interop services
- [ ] Transaction structure and validation
- [ ] Block structure and validation
- [ ] Witness verification
- [ ] Storage key/value serialization
- [ ] Contract manifest parsing

---

### Task 5: neo-json vs Neo.Json
**Files to Compare**:
- Rust: `neo-json/src/`
- C#: `neo_csharp/src/Neo.Json/`

**Audit Checklist**:
- [ ] JToken types (JObject, JArray, JString, JNumber, JBoolean, JNull)
- [ ] JSON parsing and serialization
- [ ] JPath query support

---

### Task 6: neo-io vs Neo.IO
**Files to Compare**:
- Rust: `neo-io/src/`
- C#: `neo_csharp/src/Neo.IO/`

**Audit Checklist**:
- [ ] BinaryReader/BinaryWriter equivalents
- [ ] VarInt encoding/decoding
- [ ] ISerializable trait implementation
- [ ] Memory caching patterns

---

### Task 7: neo-rpc vs RpcClient/RpcServer
**Files to Compare**:
- Rust: `neo-rpc/src/` (client/, server/)
- C#: `neo_csharp/src/RpcClient/`, `Plugins/RpcServer/`

**Audit Checklist**:
- [ ] All 39 RPC methods (Node, Blockchain, State, Wallet APIs)
- [ ] Request/Response models
- [ ] Error handling
- [ ] Transaction signing flow

---

### Task 8: neo-storage vs LevelDBStore
**Files to Compare**:
- Rust: `neo-storage/src/`
- C#: `neo_csharp/src/Plugins/LevelDBStore/`

**Audit Checklist**:
- [ ] Store interface (Get, Put, Delete, Seek)
- [ ] Snapshot support
- [ ] Batch write operations
- [ ] Key prefix conventions

---

### Task 9: neo-state vs StateService
**Files to Compare**:
- Rust: `neo-state/src/`
- C#: `neo_csharp/src/Plugins/StateService/`

**Audit Checklist**:
- [ ] State root calculation
- [ ] State proof generation/verification
- [ ] Snapshot management
- [ ] State synchronization

---

### Task 10: neo-p2p vs Neo/Network/P2P
**Files to Compare**:
- Rust: `neo-p2p/src/`
- C#: `neo_csharp/src/Neo/Network/P2P/`

**Audit Checklist**:
- [ ] Message types (Version, Verack, GetHeaders, Headers, GetBlocks, Inv, etc.)
- [ ] Peer discovery and management
- [ ] Connection handshake
- [ ] Message serialization

---

## Execution Strategy

### Phase 1: Core Components (Week 1-2)
- Task 1: neo-vm (critical for smart contract execution)
- Task 2: neo-crypto (critical for security)
- Task 3: neo-consensus (critical for block production)

### Phase 2: Protocol Layer (Week 3-4)
- Task 4: neo-core (largest module)
- Task 5: neo-json
- Task 6: neo-io

### Phase 3: Services (Week 5-6)
- Task 7: neo-rpc
- Task 8: neo-storage
- Task 9: neo-state
- Task 10: neo-p2p

---

## Deliverables

For each task:
1. **Gap Report**: Missing APIs, incorrect implementations
2. **Compatibility Matrix**: API mapping between Rust and C#
3. **Fix Recommendations**: Specific code changes needed
4. **Test Coverage**: Unit tests validating C# compatibility

---

## Success Criteria

- [ ] All public APIs in C# have Rust equivalents
- [ ] All algorithms produce identical outputs for same inputs
- [ ] All serialization formats are byte-compatible
- [ ] Test coverage ≥90% for audited modules
