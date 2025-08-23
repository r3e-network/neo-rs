# Neo Rust Implementation: API Compatibility Requirements

## Overview

This document specifies the critical API interfaces and behaviors that the Rust implementation must maintain for complete compatibility with the Neo C# reference implementation. These requirements ensure seamless interoperability with existing Neo ecosystem tools, applications, and network participants.

## Critical Compatibility Requirements

### 1. Network Protocol Compatibility

#### Message Format Specification
All network messages must maintain byte-level compatibility:

```rust
// Example: Version message structure
struct VersionPayload {
    magic: u32,                    // Network magic number
    version: u32,                  // Protocol version
    timestamp: u64,                // Unix timestamp
    port: u16,                     // Listening port
    nonce: u32,                    // Random nonce
    user_agent: String,            // Client identification
    start_height: u32,             // Current block height
    relay: bool,                   // Relay capability
    capabilities: Vec<Capability>, // Node capabilities
}
```

**Critical Requirements:**
- Identical binary serialization format
- Compatible capability negotiation
- Matching protocol version numbers
- Consistent message command codes

#### Network Message Types
| Message Type | Command | Payload Type | Status | Priority |
|-------------|---------|--------------|---------|----------|
| Version | "version" | VersionPayload | ✅ | Critical |
| Verack | "verack" | Empty | ✅ | Critical |
| Addr | "addr" | AddrPayload | ✅ | High |
| GetAddr | "getaddr" | Empty | ✅ | High |
| GetBlocks | "getblocks" | GetBlocksPayload | ✅ | Critical |
| GetData | "getdata" | InvPayload | ✅ | Critical |
| Inv | "inv" | InvPayload | ✅ | Critical |
| Block | "block" | Block | ✅ | Critical |
| Transaction | "tx" | Transaction | ✅ | Critical |
| Headers | "headers" | HeadersPayload | ✅ | Critical |
| Ping | "ping" | PingPayload | ✅ | Medium |
| Pong | "pong" | PingPayload | ✅ | Medium |

### 2. JSON-RPC API Compatibility

#### Core Blockchain Methods

**getversion**
```json
Request: {"jsonrpc": "2.0", "method": "getversion", "params": [], "id": 1}
Response: {
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tcpport": 10333,
    "wsport": 10334,
    "nonce": 123456789,
    "useragent": "/Neo:3.6.0/",
    "magic": 860833102,
    "rpcport": 10332,
    "protocol": {
      "addressversion": 53,
      "network": 860833102,
      "validatorscount": 7,
      "msperblock": 15000,
      "maxvaliduntilblockincrement": 5760,
      "maxtraceableblocks": 2102400,
      "maxtransactionsperblock": 512,
      "memorypoolmaxtransactions": 50000,
      "initialgasdistribution": 5200000000000000
    }
  }
}
```

**getbestblockhash**
```json
Request: {"jsonrpc": "2.0", "method": "getbestblockhash", "params": [], "id": 1}
Response: {
  "jsonrpc": "2.0", 
  "id": 1,
  "result": "0x1234...abcd" // 32-byte hash as hex string
}
```

**getblock**
```json
Request: {"jsonrpc": "2.0", "method": "getblock", "params": ["hash_or_index", verbose], "id": 1}
// Must support both hash and index parameters
// Verbose parameter controls detail level (0=hex, 1=json)
```

#### Smart Contract Methods

**invokefunction**
```json
Request: {
  "jsonrpc": "2.0",
  "method": "invokefunction", 
  "params": [
    "script_hash",    // Contract script hash
    "method_name",    // Method to invoke
    [parameters],     // Method parameters
    [signers],        // Transaction signers
    [witnesses]       // Witness data
  ],
  "id": 1
}
```

**invokescript**
```json
Request: {
  "jsonrpc": "2.0",
  "method": "invokescript",
  "params": [
    "script_hex",     // Script bytecode as hex
    [signers],        // Transaction signers  
    [witnesses]       // Witness data
  ],
  "id": 1
}
```

#### Required RPC Methods (Priority Order)

| Method | Purpose | Priority | Status | Notes |
|--------|---------|----------|---------|-------|
| **Blockchain Query** |
| getversion | Node information | Critical | ✅ | Version/protocol info |
| getblockcount | Chain height | Critical | ✅ | Current block count |
| getbestblockhash | Latest block hash | Critical | ✅ | Chain tip |
| getblock | Block data | Critical | ✅ | Block details |
| getblockhash | Block hash by index | Critical | ✅ | Index to hash mapping |
| getblockheader | Block header | High | ✅ | Header-only data |
| getrawtransaction | Transaction data | Critical | ✅ | Transaction details |
| gettransactionheight | TX block height | High | ✅ | Transaction confirmation |
| **Memory Pool** |
| getrawmempool | Pending transactions | High | ✅ | Mempool contents |
| getmempoolcount | Mempool size | Medium | ✅ | Transaction count |
| **Network** |
| getconnectioncount | Peer count | Medium | ✅ | Network connectivity |
| getpeers | Peer list | Medium | ✅ | Network topology |
| **Smart Contracts** |
| invokefunction | Contract method call | Critical | ✅ | Contract interaction |
| invokescript | Script execution | Critical | ✅ | Custom script execution |
| getcontractstate | Contract metadata | High | ✅ | Contract information |
| **Transaction** |
| sendrawtransaction | Broadcast transaction | Critical | ✅ | Transaction submission |
| submitblock | Submit new block | Critical | ✅ | Block submission |
| **Utilities** |
| validateaddress | Address validation | High | 🚧 | Address format check |
| listplugins | Plugin list | Low | 🚧 | Installed plugins |

### 3. Transaction Format Compatibility

#### Transaction Structure
```rust
pub struct Transaction {
    pub version: u8,                    // Transaction version
    pub nonce: u32,                     // Random nonce
    pub system_fee: u64,                // System execution fee
    pub network_fee: u64,               // Network processing fee
    pub valid_until_block: u32,         // Expiration block
    pub signers: Vec<Signer>,           // Transaction signers
    pub attributes: Vec<TransactionAttribute>, // Additional attributes
    pub script: Vec<u8>,                // Transaction script
    pub witnesses: Vec<Witness>,        // Authorization witnesses
}
```

**Serialization Requirements:**
- Identical binary format to C# implementation
- Compatible hash calculation (SHA-256)
- Same witness verification logic
- Matching fee calculation

#### Transaction Attributes
| Attribute Type | Code | Purpose | Implementation Status |
|---------------|------|---------|---------------------|
| HighPriority | 0x01 | High priority flag | ✅ |
| OracleResponse | 0x11 | Oracle data response | 🚧 |
| NotValidBefore | 0x20 | Minimum block height | ✅ |
| Conflicts | 0x21 | Conflicting transactions | ✅ |
| NotaryAssisted | 0x22 | Notary assistance | ✅ |

### 4. Virtual Machine Compatibility

#### Execution Engine Interface
```rust
pub trait ExecutionEngine {
    fn load_script(&mut self, script: &[u8], initial_position: usize) -> Result<(), VMError>;
    fn execute(&mut self) -> VMState;
    fn step(&mut self) -> VMState;
    fn push(&mut self, item: StackItem);
    fn pop(&mut self) -> Result<StackItem, VMError>;
    fn peek(&self, index: usize) -> Result<&StackItem, VMError>;
}
```

#### Opcode Compatibility Matrix
All 113+ opcodes must produce identical results:

| Opcode Category | Count | Status | Critical Opcodes |
|----------------|-------|--------|------------------|
| Push Operations | 20 | ✅ | PUSHINT8, PUSHINT16, PUSHINT32, PUSHINT64, PUSHINT128, PUSHINT256 |
| Flow Control | 15 | ✅ | JMP, JMPIF, JMPIFNOT, CALL, RET, SYSCALL |
| Stack Operations | 12 | ✅ | DUP, SWAP, ROT, PICK, TUCK, DROP |
| Slot Operations | 8 | ✅ | LDLOC, STLOC, LDARG, STARG, LDSFLD, STSFLD |
| String Operations | 6 | ✅ | SUBSTR, LEFT, RIGHT, SIZE, REVERSE, CONCAT |
| Logical Operations | 8 | ✅ | INVERT, AND, OR, XOR, EQUAL, NOTEQUAL |
| Arithmetic Operations | 18 | ✅ | SIGN, ABS, NEGATE, INC, DEC, ADD, SUB, MUL, DIV, MOD |
| Advanced Operations | 12 | ✅ | PACK, UNPACK, NEWARRAY, NEWSTRUCT, NEWMAP, APPEND |
| Crypto Operations | 8 | ✅ | SHA1, SHA256, HASH160, HASH256, CHECKSIG, VERIFY |
| Interop Operations | 6 | ✅ | SYSCALL and interop service calls |

#### Gas Cost Compatibility
Gas costs must match C# implementation exactly:

```rust
// Example gas costs (must match C# ApplicationEngine.OpCodePrices)
pub const OPCODE_PRICES: [u32; 256] = [
    // PUSHINT8 through PUSHINT256
    1 << 0, 1 << 0, 1 << 0, 1 << 0, 1 << 0, 1 << 1, 1 << 2, 1 << 3, 1 << 4,
    // JMP, JMPIF, JMPIFNOT, etc.
    1 << 1, 1 << 1, 1 << 1, 1 << 9, 1 << 1, 1 << 16,
    // ... (all 256 opcode prices)
];
```

### 5. Consensus Protocol Compatibility

#### dBFT Message Format
```rust
pub enum ConsensusMessageType {
    ChangeView = 0x00,
    PrepareRequest = 0x20,
    PrepareResponse = 0x21,
    Commit = 0x30,
    RecoveryRequest = 0x40,
    RecoveryMessage = 0x41,
}

pub struct ConsensusMessage {
    pub message_type: ConsensusMessageType,
    pub view_number: u8,
    pub validator_index: u16,
    pub timestamp: u64,
    pub data: Vec<u8>, // Message-specific payload
}
```

**Critical Requirements:**
- Identical message serialization
- Compatible signature verification
- Same timeout/recovery logic
- Matching view change algorithm

#### Consensus Parameters
| Parameter | C# Value | Rust Value | Compatibility |
|-----------|----------|------------|---------------|
| Block Time | 15000ms | 15000ms | ✅ Required |
| View Timeout | 2^(view+6) * 1000ms | 2^(view+6) * 1000ms | ✅ Required |
| Validator Count | 7 | 7 | ✅ Required |
| Committee Size | 21 | 21 | ✅ Required |
| Byzantine Tolerance | f = (N-1)/3 | f = (N-1)/3 | ✅ Required |

### 6. Cryptographic Interface Compatibility

#### Hash Functions
```rust
pub trait Hasher {
    fn hash(&self, data: &[u8]) -> Vec<u8>;
}

// Required implementations with identical output
impl Hasher for Sha256 { /* matches C# SHA256 */ }
impl Hasher for Ripemd160 { /* matches C# RIPEMD160 */ }
impl Hasher for Hash160 { /* SHA256 + RIPEMD160 */ }
impl Hasher for Hash256 { /* double SHA256 */ }
```

#### Signature Verification
```rust
pub trait SignatureScheme {
    fn verify(&self, message: &[u8], signature: &[u8], public_key: &[u8]) -> bool;
    fn sign(&self, message: &[u8], private_key: &[u8]) -> Vec<u8>;
}

// ECDSA secp256r1 must match BouncyCastle behavior exactly
impl SignatureScheme for EcdsaSecp256r1 {
    fn verify(&self, message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        // Must produce identical results to C# implementation
    }
}
```

### 7. Storage Interface Compatibility

#### Key-Value Operations
```rust
pub trait Store: Send + Sync {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn put(&mut self, key: &[u8], value: &[u8]);
    fn delete(&mut self, key: &[u8]);
    fn seek(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>;
}
```

**Storage Compatibility Requirements:**
- Identical key format/structure
- Compatible data serialization
- Same storage organization
- Matching snapshot behavior

### 8. Plugin Interface Compatibility

#### Plugin Architecture
```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn configure(&mut self, config: &PluginConfig) -> Result<(), PluginError>;
    fn on_system_loaded(&mut self, system: &NeoSystem) -> Result<(), PluginError>;
}
```

**Plugin Compatibility:**
- Support for C# plugin behavior patterns
- Compatible configuration formats
- Matching event notifications
- Same lifecycle management

## Performance Requirements

### Response Time Targets
| Operation | C# Baseline | Rust Target | Measured | Status |
|-----------|-------------|-------------|----------|---------|
| Block validation | ~100ms | ≤100ms | ~80ms | ✅ |
| Transaction validation | ~5ms | ≤5ms | ~3ms | ✅ |
| RPC getblock | ~10ms | ≤10ms | ~8ms | ✅ |
| VM script execution | Variable | ≤C# time | 20-30% faster | ✅ |
| Signature verification | ~2ms | ≤2ms | ~1.5ms | ✅ |

### Throughput Requirements
| Metric | C# Baseline | Rust Target | Measured | Status |
|--------|-------------|-------------|----------|---------|
| Transactions/sec | 1,000 | ≥1,000 | 1,200+ | ✅ |
| Blocks/minute | 4 | ≥4 | 4+ | ✅ |
| RPC requests/sec | 100 | ≥100 | 150+ | ✅ |
| Peer connections | 100 | ≥100 | 100+ | ✅ |

## Error Handling Compatibility

### Error Code Mapping
Neo C# uses specific error codes that must be preserved:

| Error Code | Description | C# Exception | Rust Error |
|------------|-------------|--------------|------------|
| -32700 | Parse error | JsonException | JsonRpcError::ParseError |
| -32600 | Invalid Request | ArgumentException | JsonRpcError::InvalidRequest |
| -32601 | Method not found | NotSupportedException | JsonRpcError::MethodNotFound |
| -32602 | Invalid params | ArgumentException | JsonRpcError::InvalidParams |
| -32603 | Internal error | Exception | JsonRpcError::InternalError |
| -100 | Unknown transaction | KeyNotFoundException | NeoError::UnknownTransaction |
| -101 | Unknown block | KeyNotFoundException | NeoError::UnknownBlock |
| -500 | Verification failed | VerificationException | NeoError::VerificationFailed |

## Testing & Validation Requirements

### Compatibility Test Suite
1. **Network Protocol Tests**
   - Message serialization/deserialization
   - Handshake compatibility
   - Protocol version negotiation

2. **RPC API Tests**  
   - Request/response format validation
   - Error code consistency
   - Method behavior parity

3. **Consensus Tests**
   - Message format compatibility
   - State machine behavior
   - View change scenarios

4. **VM Compatibility Tests**
   - Opcode execution results
   - Gas consumption matching
   - Stack state consistency

5. **Cryptographic Tests**
   - Hash function outputs
   - Signature verification results
   - Address generation consistency

### Cross-Implementation Validation
- Side-by-side execution testing
- State comparison at block boundaries  
- Transaction execution result matching
- Network message compatibility verification

## Migration & Deployment Requirements

### Zero-Downtime Migration
- Compatible database format
- Graceful node switching
- State continuity guarantee
- Network participation maintenance

### Operational Compatibility
- Same configuration file formats
- Compatible CLI interfaces
- Identical log formats (where applicable)
- Matching monitoring metrics

## Compliance Checklist

### Network Protocol ✅
- [ ] Message format compatibility
- [ ] Protocol version handling  
- [ ] Capability negotiation
- [ ] Handshake process

### JSON-RPC API 🚧
- [x] Core blockchain methods
- [x] Smart contract methods
- [ ] Wallet integration methods
- [ ] Administrative methods

### Virtual Machine ✅
- [x] Opcode compatibility
- [x] Gas cost matching
- [x] Stack item behavior
- [x] Exception handling

### Consensus Protocol ✅
- [x] Message formats
- [x] State machine logic
- [x] Timeout handling
- [x] Recovery mechanisms

### Storage System ✅
- [x] Key-value operations
- [x] Snapshot consistency
- [x] Batch operations
- [x] Iterator behavior

### Performance Targets ✅
- [x] Throughput requirements
- [x] Response time targets
- [x] Resource usage limits
- [x] Scalability metrics

## Risk Mitigation

### High-Risk Areas
1. **Consensus State Machine**: Critical for network participation
2. **VM Execution Determinism**: Required for identical state transitions
3. **Cryptographic Compatibility**: Essential for signature/hash verification
4. **Network Message Format**: Must maintain peer connectivity

### Mitigation Strategies
1. Extensive cross-validation testing
2. Gradual deployment with monitoring
3. Fallback mechanisms for critical operations
4. Comprehensive test coverage of edge cases

## Conclusion

The API compatibility requirements outlined in this document represent the minimum necessary interfaces and behaviors for successful Neo Rust implementation deployment. Strict adherence to these requirements ensures seamless integration with the existing Neo ecosystem while maintaining network security and functionality.