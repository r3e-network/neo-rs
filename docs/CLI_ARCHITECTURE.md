# CLI Architecture and Wallet Operations

## Current Design

The `neo-cli` and `neo-node` follow a **client-server architecture**:

```
┌─────────────┐                  ┌─────────────────┐
│  neo-cli   │  JSON-RPC   │  neo-node      │
│  (Client)   │ ◄──────────►│  (Server)       │
└─────────────┘                  └─────────────────┘
     │                                     │
     │ Local parsing               Wallet │ Storage
     │                            │         │
     │                            └─────────┘
```

## Architecture Comparison

### C# CLI (Embedded)

```csharp
// neo-csharp/node/src/Neo.CLI/CLI/MainService.cs

public class MainService
{
    private Wallet CurrentWallet { get; set; }

    // Direct wallet operations
    [ConsoleCommand("open wallet")]
    private void OnOpenWallet(string path) {
        // Load wallet file directly
        CurrentWallet = Wallet.Open(path, password);
    }

    [ConsoleCommand("transfer")]
    private void OnTransferCommand(...) {
        // Sign transaction locally
        var tx = CurrentWallet.MakeTransaction(...);
        // Submit directly to blockchain
        NeoSystem.Blockchain.AddTransaction(tx);
    }
}
```

**Characteristics:**
- ✅ Works offline (no node required)
- ✅ Direct file system access
- ✅ Can create/sign transactions without network
- ⚠️ Larger binary (includes full blockchain logic)

### Rust CLI (RPC Client)

```rust
// neo-cli/src/main.rs

#[tokio::main]
async fn main() -> Result<()> {
    let client = RpcClient::new(rpc_url)?;

    // All operations go through RPC
    match command {
        Commands::Block { index } => {
            let block = client.get_block(&index).await?;
            println!("{}", block);
        }
        Commands::Wallet { cmd } => {
            match cmd {
                WalletCommands::Open { path } => {
                    // RPC-based wallet operation
                    client.open_wallet(path).await?;
                }
            }
        }
    }
}
```

**Characteristics:**
- ✅ Smaller binary (~5MB vs ~60MB)
- ✅ Clear separation of concerns
- ✅ Can run on different machines from node
- ⚠️ Requires running neo-node for most operations
- ⚠️ Network-dependent for wallet operations

## Command Mapping

### Blockchain Queries

| C# Command | Rust CLI Command | Implementation |
|-------------|-------------------|------------------|
| `show block <hash|index>` | `block <hash|index>` | ✅ Full RPC support |
| `show tx <hash>` | `tx <hash>` | ✅ Full RPC support |
| `state` | `state` | ✅ Full RPC support |
| `best block hash` | `best_block_hash` | ✅ Full RPC support |

**Status:** Fully implemented via RPC

### Wallet Operations

| C# Command | Rust CLI Command | Status |
|-------------|-------------------|--------|
| `open wallet` | `wallet open` | ⚠️ Requires RPC implementation |
| `close wallet` | `wallet close` | ⚠️ Requires RPC implementation |
| `create wallet` | `wallet create` | ⚠️ Requires RPC implementation |
| `create address` | `wallet create-address` | ⚠️ Requires RPC implementation |
| `list address` | `wallet list` | ⚠️ Requires RPC implementation |
| `send` | `send` | ✅ Via `sendrawtransaction` RPC |
| `transfer` | `invoke` (contract call) | ✅ Via `invokecontract` RPC |

**Current Implementation:** Most wallet operations show "not yet implemented via RPC"

### Consensus & Plugins

| C# Command | Rust CLI Command | Status |
|-------------|-------------------|--------|
| `start consensus` | `consensus start` | ✅ Via `startconsensus` RPC |
| `start oracle` | `oracle start` | ⚠️ RPC method exists, needs CLI wiring |
| `list plugins` | `plugins` | ✅ Via `listplugins` RPC |

## Design Rationale

### Why RPC-Based CLI?

1. **Single Source of Truth**
   - All blockchain state comes from neo-node
   - Prevents inconsistencies between CLI and node

2. **Modularity**
   - CLI can be upgraded independently
   - CLI can run on different machine
   - Multiple CLI instances can share same node

3. **Binary Size**
   - neo-cli: ~5MB
   - neo-csharp: ~60MB
   - Faster downloads and updates

4. **Security**
   - No direct wallet file access from untrusted machines
   - Wallet stays on trusted node machine
   - Authenticated RPC for all operations

5. **Maintenance**
   - Single implementation in neo-node
   - No code duplication between CLI and node
   - Easier to audit

## Offline Operation Scenarios

### Scenario 1: Prepare Transaction Offline

**C# Approach:**
```bash
# Open wallet offline
neo-cli open wallet wallet.json
# Create transaction offline
neo-cli transfer <to> <amount>
# Save signed transaction
neo-cli export tx output.json
# Later: Submit from different machine
neo-cli submit output.json
```

**Current Rust Limitation:**
```bash
# This flow is not currently supported
neo-cli wallet create # ❌ Not implemented
neo-cli wallet open # ❌ Requires running node
```

### Scenario 2: Multi-Signature Setup

**C# Approach:**
```bash
# Each signer creates their part
neo-cli sign tx.json
# Combine offline
neo-cli combine-sigs part1.json part2.json
# Submit
neo-cli submit combined.json
```

**Current Rust Limitation:**
- Multi-sig transactions require neo-node to be running
- Each signer needs network access to node

## Roadmap: Enhanced Wallet Support

### Option 1: RPC-Based Full Wallet API (Recommended)

**Pros:**
- Maintains current architecture
- Single implementation point
- Works across network boundaries

**Implementation:**
```rust
// neo-rpc/src/server/wallet_api.rs (extend)

impl RpcServerWallet {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            // Existing methods...
            Self::handler("openwallet", Self::open_wallet),
            Self::handler("closewallet", Self::close_wallet),
            Self::handler("createwallet", Self::create_wallet),
            Self::handler("importkey", Self::import_key),
            Self::handler("createaddress", Self::create_address),
        ]
    }

    fn open_wallet(server: &RpcServer, params: &[Value])
        -> Result<Value, RpcException>
    {
        let path = Self::expect_string_param(params, 0, "openwallet")?;
        let password = Self::expect_string_param(params, 1, "openwallet")?;

        // Store wallet reference in neo-node
        // CLI can then use wallet: parameter
        Ok(json!("success"))
    }
}
```

**CLI Changes:**
```rust
// neo-cli/src/commands/wallet.rs

pub async fn execute(_client: &RpcClient, cmd: WalletCommands)
    -> CommandResult
{
    match cmd {
        WalletCommands::Open { path, password } => {
            let result = client.rpc_send_async(
                "openwallet",
                vec![json!(path), json!(password)]
            ).await?;
            Ok("Wallet opened successfully on node.".to_string())
        }
        // ... other commands ...
    }
}
```

### Option 2: Standalone Wallet Library

**Pros:**
- Enables true offline operation
- Can be reused in other Rust projects

**Implementation:**
```rust
// Create new crate: neo-wallet-cli

pub struct WalletManager {
    wallet: Option<Wallet>,
}

impl WalletManager {
    pub fn open(&mut self, path: &str, password: &str)
        -> Result<()>
    {
        self.wallet = Some(Wallet::load(path, password)?);
        Ok(())
    }

    pub fn create_transaction(&self, to: &Address, amount: BigDecimal)
        -> Result<Transaction>
    {
        let wallet = self.wallet.as_ref()?;
        let script = build_transfer_script(to, amount)?;
        let tx = wallet.sign(script)?;
        Ok(tx)
    }

    pub fn export_tx(&self, tx: &Transaction) -> Result<String> {
        Ok(serde_json::to_string_pretty(tx)?)
    }

    pub fn import_tx(&mut self, json: &str) -> Result<Transaction> {
        Ok(serde_json::from_str(json)?)
    }
}
```

### Option 3: Hybrid Approach

Combine both:
- Use RPC for blockchain queries
- Use local wallet for signing
- Submit via RPC

```rust
// neo-cli can have wallet feature
#[cfg(feature = "local-wallet")]
mod local_wallet;

#[cfg(not(feature = "local-wallet"))]
mod rpc_wallet;

// Choose implementation at runtime based on feature
```

## Recommendations

### Short Term (v0.7.x)

1. **Document current limitations** - Add to README
2. **Implement RPC wallet API** - Add methods to neo-rpc server
3. **Wire up CLI commands** - Connect CLI to RPC methods
4. **Add offline signing** - Optional feature for advanced users

### Long Term (v0.8+)

1. **Evaluate demand for offline operation**
   - If high demand: Implement standalone wallet library
   - If low demand: Enhance RPC-based approach

2. **Hardware wallet support**
   - Integrate with neo-hsm
   - CLI can sign via HSM

3. **Multi-sig workflows**
   - Split key support
   - Partial signing

## Migration Guide

### For Users Currently Using C# CLI

**Online operations** - No changes needed:
```bash
# C# (current)
neo-cli state
neo-cli block 1000

# Rust (equivalent)
neo-cli state
neo-cli block 1000
```

**Wallet operations** - Adapt to RPC model:
```bash
# C# (current)
neo-cli open wallet mywallet.json
neo-cli transfer 0x123... 10 NEO

# Rust (adapted)
# 1. Open wallet on neo-node (once)
neo-cli wallet open mywallet.json
# 2. Then use wallet implicitly
neo-cli send 0x123... 10 NEO
```

### For Operators

**No immediate action required:**
- Current architecture is production-ready
- Most common operations are fully supported

**Future enhancement:**
- Implement RPC wallet API for enhanced CLI
- Consider offline wallet library based on user demand

## Conclusion

The RPC-based CLI is an **architectural improvement** that provides:
- ✅ Better separation of concerns
- ✅ Smaller deployment footprint
- ✅ Network scalability

The limitation on local wallet operations is **not a bug** but a **design choice** aligned with modern client-server patterns.

**Enhancement path is clear** and can be implemented via RPC methods without architectural changes.
