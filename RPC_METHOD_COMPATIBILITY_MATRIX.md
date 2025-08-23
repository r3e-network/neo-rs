# Neo RPC Method Compatibility Matrix

## Complete C# to Rust RPC Method Mapping

This document provides a detailed method-by-method compatibility analysis between the C# Neo RPC server and the current Rust implementation.

## Legend
- ✅ **Fully Compatible** - Method implemented with 100% compatibility
- ⚠️ **Partially Compatible** - Method implemented but missing features or different behavior
- ❌ **Not Implemented** - Method completely missing from Rust implementation
- 🔄 **In Progress** - Method currently under development

---

## Blockchain Query Methods

### Block Operations

| Method | C# Signature | Rust Status | Compatibility | Notes |
|--------|-------------|-------------|---------------|-------|
| `getbestblockhash` | `GetBestBlockHash()` | ✅ | 100% | Perfect match |
| `getblock` | `GetBlock(BlockHashOrIndex, bool verbose = false)` | ⚠️ | 85% | Missing confirmations field |
| `getblockcount` | `GetBlockCount()` | ✅ | 100% | Perfect match |
| `getblockhash` | `GetBlockHash(uint height)` | ✅ | 100% | Perfect match |
| `getblockheader` | `GetBlockHeader(BlockHashOrIndex, bool verbose = false)` | ❌ | 0% | **MISSING - High Priority** |
| `getblockheadercount` | `GetBlockHeaderCount()` | ❌ | 0% | **MISSING - Medium Priority** |

**C# Example Usage:**
```csharp
// getblockheader - MISSING in Rust
var header = rpc.GetBlockHeader("0x1234...", verbose: true);
// Response includes: hash, size, version, previousblockhash, merkleroot, time, nonce, index, primary, nextconsensus, witnesses, confirmations, nextblockhash
```

### Transaction Operations

| Method | C# Signature | Rust Status | Compatibility | Notes |
|--------|-------------|-------------|---------------|-------|
| `getrawtransaction` | `GetRawTransaction(UInt256 hash, bool verbose = false)` | ❌ | 0% | **MISSING - Critical** |
| `getrawmempool` | `GetRawMemPool(bool shouldGetUnverified = false)` | ❌ | 0% | **MISSING - High Priority** |
| `gettransactionheight` | `GetTransactionHeight(UInt256 hash)` | ❌ | 0% | **MISSING - Medium Priority** |

**C# Example Usage:**
```csharp
// getrawtransaction - MISSING in Rust
var tx = rpc.GetRawTransaction("0xabcd...", verbose: true);
// Response includes: hash, size, version, nonce, sender, sysfee, netfee, validuntilblock, attributes, signers, script, witnesses, blockhash, confirmations, blocktime
```

### Contract State Operations

| Method | C# Signature | Rust Status | Compatibility | Notes |
|--------|-------------|-------------|---------------|-------|
| `getcontractstate` | `GetContractState(ContractNameOrHashOrId)` | ❌ | 0% | **MISSING - High Priority** |
| `getstorage` | `GetStorage(ContractNameOrHashOrId, string base64Key)` | ❌ | 0% | **MISSING - Critical** |
| `findstorage` | `FindStorage(ContractNameOrHashOrId, string base64KeyPrefix, int start = 0)` | ❌ | 0% | **MISSING - Critical** |

**C# Example Usage:**
```csharp
// getcontractstate - MISSING in Rust
var state = rpc.GetContractState("0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5");
// Returns full contract manifest, NEF, and metadata

// getstorage - MISSING in Rust
var value = rpc.GetStorage("0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5", "dG90YWxTdXBwbHk=");
// Returns Base64-encoded storage value
```

### Validator Operations

| Method | C# Signature | Rust Status | Compatibility | Notes |
|--------|-------------|-------------|---------------|-------|
| `getnextblockvalidators` | `GetNextBlockValidators()` | ❌ | 0% | **MISSING - Medium Priority** |
| `getcandidates` | `GetCandidates()` | ❌ | 0% | **MISSING - Medium Priority** |
| `getcommittee` | `GetCommittee()` | ❌ | 0% | **MISSING - Medium Priority** |

---

## Smart Contract Methods

| Method | C# Signature | Rust Status | Compatibility | Notes |
|--------|-------------|-------------|---------------|-------|
| `invokefunction` | `InvokeFunction(UInt160 scriptHash, string operation, ContractParameter[] args, SignersAndWitnesses signersAndWitnesses, bool useDiagnostic = false)` | ❌ | 0% | **MISSING - Critical** |
| `invokescript` | `InvokeScript(byte[] script, SignersAndWitnesses signersAndWitnesses, bool useDiagnostic = false)` | ❌ | 0% | **MISSING - Critical** |
| `getunclaimedgas` | `GetUnclaimedGas(Address address)` | ❌ | 0% | **MISSING - High Priority** |
| `traverseiterator` | `TraverseIterator(Guid sessionId, Guid iteratorId, int count)` | ❌ | 0% | **MISSING - High Priority** |
| `terminatesession` | `TerminateSession(Guid sessionId)` | ❌ | 0% | **MISSING - High Priority** |

**C# Example Usage:**
```csharp
// invokefunction - MISSING in Rust  
var result = rpc.InvokeFunction("0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5", 
    "balanceOf", 
    new ContractParameter[] { 
        new ContractParameter(ContractParameterType.Hash160, "0x1234...")
    });
// Returns: script, state, gasconsumed, stack, notifications, diagnostics
```

---

## Node Management Methods

| Method | C# Signature | Rust Status | Compatibility | Notes |
|--------|-------------|-------------|---------------|-------|
| `getversion` | `GetVersion()` | ⚠️ | 80% | Missing RPC settings and hardforks |
| `getconnectioncount` | `GetConnectionCount()` | ✅ | 100% | Perfect match |
| `getpeers` | `GetPeers()` | ✅ | 100% | Perfect match |
| `sendrawtransaction` | `SendRawTransaction(string base64Tx)` | ❌ | 0% | **MISSING - Critical** |
| `submitblock` | `SubmitBlock(string base64Block)` | ❌ | 0% | **MISSING - High Priority** |

**C# Example - Missing getversion fields:**
```csharp
// Current Rust implementation missing:
{
  "rpc": {
    "maxiteratorresultitems": 100,
    "sessionenabled": false
  },
  "protocol": {
    "hardforks": [
      {"name": "Aspidochelone", "blockheight": 0},
      {"name": "Basilisk", "blockheight": 1730000}
    ],
    "standbycommittee": ["0x..."],
    "seedlist": ["seed1.neo.org:10333"]
  }
}
```

---

## Wallet Methods

**ALL WALLET METHODS ARE MISSING** - This represents a major functionality gap.

| Method | C# Signature | Rust Status | Priority | Notes |
|--------|-------------|-------------|----------|-------|
| `closewallet` | `CloseWallet()` | ❌ | High | Core wallet operation |
| `dumpprivkey` | `DumpPrivKey(Address address)` | ❌ | Medium | Security sensitive |
| `getnewaddress` | `GetNewAddress()` | ❌ | High | Core wallet operation |
| `getwalletbalance` | `GetWalletBalance(UInt160 assetId)` | ❌ | High | Essential for UX |
| `getwalletunclaimedgas` | `GetWalletUnclaimedGas()` | ❌ | Medium | GAS mechanics |
| `importprivkey` | `ImportPrivKey(string privkey)` | ❌ | Medium | Wallet management |
| `calculatenetworkfee` | `CalculateNetworkFee(byte[] tx)` | ❌ | High | Fee estimation |
| `listaddress` | `ListAddress()` | ❌ | Medium | Wallet overview |
| `openwallet` | `OpenWallet(string path, string password)` | ❌ | Critical | Core wallet operation |
| `sendfrom` | `SendFrom(UInt160 assetId, Address from, Address to, string amount, Address[] signers = null)` | ❌ | High | Transaction creation |
| `sendmany` | `SendMany(JArray _params)` | ❌ | Medium | Batch transactions |
| `sendtoaddress` | `SendToAddress(UInt160 assetId, Address to, string amount)` | ❌ | High | Basic transfer |
| `canceltransaction` | `CancelTransaction(UInt256 txid, Address[] signers, string extraFee = null)` | ❌ | Low | Advanced feature |
| `invokecontractverify` | `InvokeContractVerify(UInt160 scriptHash, ContractParameter[] args, SignersAndWitnesses signersAndWitnesses)` | ❌ | Medium | Contract interaction |

**C# Wallet Example - All MISSING in Rust:**
```csharp
// Wallet operations - ALL MISSING
rpc.OpenWallet("wallet.json", "password");
var address = rpc.GetNewAddress();
var balance = rpc.GetWalletBalance(NeoToken.Hash);
var txid = rpc.SendToAddress(NeoToken.Hash, "NXXXAddress", "10");
rpc.CloseWallet();
```

---

## Utility Methods

| Method | C# Signature | Rust Status | Compatibility | Notes |
|--------|-------------|-------------|---------------|-------|
| `validateaddress` | `ValidateAddress(string address)` | ⚠️ | 60% | Basic validation only |
| `listplugins` | `ListPlugins()` | ❌ | 0% | **MISSING - Low Priority** |

**C# validateaddress - Partially Missing in Rust:**
```csharp
// Full C# response (Rust only returns basic validation):
{
  "address": "NXXXAddress",
  "isvalid": true,
  "isscript": false,  // MISSING in Rust
  "type": "Address"   // MISSING in Rust  
}
```

---

## Native Contracts

| Method | C# Signature | Rust Status | Compatibility | Notes |
|--------|-------------|-------------|---------------|-------|
| `getnativecontracts` | `GetNativeContracts()` | ⚠️ | 40% | Returns hardcoded data |

**Issues with Current Implementation:**
- Returns static contract data instead of querying blockchain
- Missing manifest details and update history
- No dynamic contract discovery

---

## Implementation Priority Matrix

### 🔥 **CRITICAL (Week 1-2)**
1. `getrawtransaction` - Essential for transaction lookup
2. `getstorage` / `findstorage` - Critical for contract state inspection  
3. `invokefunction` - Core smart contract interaction
4. `sendrawtransaction` - Transaction broadcasting

### 🟡 **HIGH PRIORITY (Week 3-5)**  
1. `getblockheader` - Block header queries
2. `getrawmempool` - Memory pool inspection
3. `invokescript` - Script execution
4. `openwallet` / `closewallet` - Basic wallet operations
5. `sendtoaddress` - Basic token transfers
6. `getcontractstate` - Contract information

### 🟢 **MEDIUM PRIORITY (Week 6-8)**
1. `getunclaimedgas` - GAS mechanics
2. `getnextblockvalidators` - Consensus info  
3. `calculatenetworkfee` - Fee calculation
4. `getwalletbalance` - Balance queries
5. `traverseiterator` - Large result handling

### 🔵 **LOW PRIORITY (Week 9-10)**
1. `listplugins` - Plugin management
2. `canceltransaction` - Advanced wallet features
3. `terminatesession` - Session management
4. Enhanced `validateaddress` - Complete address validation

---

## Response Format Differences

### UInt160/UInt256 Serialization
```csharp
// C# - Returns hex with 0x prefix
"hash": "0x1234567890abcdef..."

// Rust - May return without prefix or different casing
"hash": "1234567890abcdef..." // INCONSISTENCY
```

### Timestamp Formats
```csharp  
// C# - Unix timestamp in milliseconds
"time": 1627896461306

// Rust - May use different precision
"time": 1627896461 // MISSING MILLISECONDS
```

### Error Response Formats
```csharp
// C# - Rich error information
{
  "error": {
    "code": -32602,
    "message": "Invalid params",
    "data": "Additional error context"  // Often missing in Rust
  }
}
```

---

## Testing Requirements for 100% Compatibility

### 1. Response Format Tests
```rust
#[test]  
fn test_response_format_matches_csharp() {
    // Every method needs format validation
    let result = rpc.call("getblock", params);
    assert_matches_csharp_format(result, "getblock_response_schema.json");
}
```

### 2. Parameter Validation Tests  
```rust
#[test]
fn test_parameter_validation_parity() {
    // Must reject same invalid inputs as C#
    assert_error_code_matches(
        rpc.call("getblockhash", vec!["invalid"]),
        -32602 // Invalid params
    );
}
```

### 3. Edge Case Tests
```rust
#[test]
fn test_edge_case_compatibility() {
    // Handle edge cases identically to C#
    // Empty blocks, genesis block, maximum values, etc.
}
```

This comprehensive matrix shows that **significant development work** is required to achieve 100% C# RPC compatibility. The current implementation covers only basic blockchain queries and lacks the smart contract, wallet, and advanced node management functionality that makes up the majority of Neo RPC usage.