# Plugin System Guide

## Overview

The Rust implementation follows a **compile-time integration** approach rather than dynamic plugin loading used in the C# version. This provides better type safety, performance, and deployment simplicity.

## Plugin Architecture Comparison

### C# (Dynamic Loading)

```csharp
// Plugin attribute marks a class for reflection-based loading
[Plugin]
public class RpcServer : Plugin
{
    protected override void Configure() { }
    protected override void OnSystemLoaded(NeoSystem system) { }
}

// Separate config file per plugin
// Plugins/RpcServer/RpcServer.json
```

**Workflow:**
1. Scan `Plugins/` directory for assemblies
2. Reflectively find `[Plugin]`-attributed classes
3. Load config from `<PluginName>.json`
4. Instantiate and inject dependencies

### Rust (Compile-time Integration)

```rust
// Feature flag enables plugin at build time
#[cfg(feature = "rpc")]
mod rpc_server;

// Feature flag in Cargo.toml
[features]
default = ["rpc", "consensus", "oracle"]
rpc = []
consensus = []
oracle = []
hsm = []
tee = []
```

**Workflow:**
1. Cargo features determine what gets compiled
2. Services registered in `neo-node` main()
3. Configuration via unified `neo-node.toml`
4. ServiceRegistry manages lifecycle

## Module Mapping

| Functionality | C# Plugin | Rust Module/Crate | Feature Flag |
|--------------|-------------|-------------------|--------------|
| **RPC Server** | `Neo.Plugins.RpcServer` | `neo-rpc` (server module) | `rpc` |
| **Consensus** | `Neo.Plugins.DBFTPlugin` | `neo-consensus` | `consensus` |
| **Oracle Service** | `Neo.Plugins.OracleService` | `neo-core::oracle_service` | `oracle` |
| **State Service** | `Neo.Plugins.StateService` | `neo-core::state_service` | `state-root` |
| **Tokens Tracker** | `Neo.Plugins.TokensTracker` | `neo-core::tokens_tracker` | Built-in |
| **Application Logs** | `Neo.Plugins.ApplicationLogs` | `neo-core::application_logs` | Built-in |
| **HSM Support** | Neo.Plugins.SignClient | `neo-hsm` + integration | `hsm` |
| **TEE Support** | (External) | `neo-tee` | `tee` |
| **NeoFS** | `OracleService` protocol | `oracle_service/neofs` | `oracle` |

## Configuration

### C# Style (Per-Plugin Config)

```json
// Plugins/RpcServer/RpcServer.json
{
  "Network": 5195086,
  "BindAddress": "127.0.0.1",
  "Port": 10332,
  "User": "",
  "Pass": ""
}

// Plugins/OracleService/OracleService.json
{
  "Network": 5195086,
  "AutoStart": true,
  "AllowedContentTypes": ["Url"],
  "MaxPrice": 100000000
}
```

### Rust Style (Unified Config)

```toml
# neo-node.toml
[rpc]
enabled = true
bind_address = "127.0.0.1"
port = 10332
user = ""
pass = ""

[oracle]
enabled = true
auto_start = true
max_price = 100000000
```

## Service Lifecycle

### C# Plugin Lifecycle

```csharp
public abstract class Plugin
{
    protected abstract void Configure();
    protected abstract void Dispose();
    protected virtual void OnSystemLoaded(NeoSystem system) { }
    protected virtual void OnSystemStarted() { }
}
```

**Sequence:**
1. `Configure()` - Load configuration
2. `OnSystemLoaded()` - Access NeoSystem services
3. `OnSystemStarted()` - System ready
4. `Dispose()` - Cleanup

### Rust Service Lifecycle

```rust
// ServiceRegistry pattern
pub struct ServiceRegistry {
    services: HashMap<TypeId, Arc<dyn Any>>,
}

// Service initialization in neo-node main()
let _application_logs_service =
    maybe_enable_application_logs(&node_config, &protocol_settings, &system)?;

let _tokens_tracker_service =
    maybe_enable_tokens_tracker(&node_config, &protocol_settings, &system)?;

let oracle_service =
    maybe_enable_oracle_service(&node_config, &protocol_settings, &system)?;
```

**Sequence:**
1. Check config enablement
2. Instantiate service with dependencies
3. Register with ServiceRegistry
4. Start background tasks
5. Drop on shutdown (RAII)

## Adding a New Service

### Step 1: Define Feature

Add to `neo-node/Cargo.toml`:

```toml
[features]
default = ["my-service"]
my-service = []
```

### Step 2: Implement Service Module

Create `neo-node/src/my_service.rs`:

```rust
use crate::config::MyServiceSettings;
use neo_core::neo_system::NeoSystem;
use std::sync::Arc;

pub struct MyService {
    system: Arc<NeoSystem>,
    settings: MyServiceSettings,
}

impl MyService {
    pub fn new(
        system: Arc<NeoSystem>,
        settings: MyServiceSettings,
    ) -> Self {
        Self { system, settings }
    }

    pub fn start(&self) -> Result<(), Error> {
        // Service logic here
        Ok(())
    }
}
```

### Step 3: Add Config

Add to `neo-node/src/config.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct MyServiceSettings {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub config_path: Option<String>,
}
```

Add to `NodeConfig`:

```rust
#[derive(Debug, Deserialize)]
pub struct NodeConfig {
    // ... existing fields ...
    #[serde(default)]
    pub my_service: MyServiceSettings,
}
```

### Step 4: Add CLI Flags

Add to `neo-node/src/cli.rs`:

```rust
#[derive(Parser, Debug)]
pub struct NodeCli {
    // ... existing flags ...

    #[clap(group = "my-service")]
    pub my_service_args: Option<MyServiceArgs>,
}

#[derive(Parser, Debug)]
pub struct MyServiceArgs {
    #[arg(long)]
    pub config: Option<PathBuf>,
}
```

### Step 5: Initialize in main()

Add to `neo-node/src/main.rs`:

```rust
#[cfg(feature = "my-service")]
mod my_service;

// ... in main() function ...
#[cfg(feature = "my-service")]
let _my_service = maybe_enable_my_service(&node_config, &system)?;

fn maybe_enable_my_service(
    config: &NodeConfig,
    system: &Arc<NeoSystem>,
) -> Result<Option<MyService>> {
    if !config.my_service.enabled {
        return Ok(None);
    }

    let service = MyService::new(system.clone(), config.my_service.clone());
    service.start()?;
    Ok(Some(service))
}
```

### Step 6: Update TOML Schema

Add to `neo-node/config.rs` or create schema validation:

```rust
pub fn validate_node_config(
    config: &NodeConfig,
    // ... other params ...
) -> Result<(), Error> {
    // ... existing validation ...

    if config.my_service.enabled {
        // Validate my-service specific settings
    }

    Ok(())
}
```

## Benefits of Rust Approach

### 1. Type Safety
- Compile-time checking prevents configuration errors
- No reflection overhead
- Better IDE support and documentation

### 2. Performance
- No dynamic assembly loading
- Zero-cost abstractions
- Inlining opportunities

### 3. Deployment
- Single binary (`neo-node`) vs multiple DLLs
- No plugin version conflicts
- Easier containerization

### 4. Security
- No untrusted code loading
- All code audited at build time
- Smaller attack surface

## Migration from C# Plugins

If you have custom C# plugins, follow this guide:

### 1. Identify Dependencies

What services does your plugin need?
- `NeoSystem` (core services)
- `IWalletProvider` (wallet access)
- `LocalNode` (P2P)
- `Blockchain` (ledger)

### 2. Port Logic

- Translate C# to Rust
- Replace Akka actors with Tokio tasks
- Use `ServiceRegistry` for dependency injection

### 3. Add Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_service_initialization() {
        // Test your service
    }
}
```

### 4. Update Documentation

- Add to `ARCHITECTURE_COMPARISON.md`
- Document config options in `README.md`
- Add migration notes

## Common Patterns

### Background Tasks

```rust
pub struct MyService {
    handle: JoinHandle<()>,
}

impl MyService {
    pub fn start(&self) -> Result<()> {
        self.handle = tokio::spawn(async move {
            loop {
                // Background work
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
        Ok(())
    }
}
```

### Event Handlers

```rust
use neo_core::i_event_handlers::{ICommittingHandler, IPersistHandler};

impl ICommittingHandler for MyService {
    fn i_blockchain_committing_handler(&self, _block: &Block) {
        // Handle block committing
    }
}

impl IPersistHandler for MyService {
    fn i_blockchain_persist_handler(&self, block: &Block) {
        // Handle block persisted
    }
}
```

### RPC Integration

```rust
impl RpcServerMyService {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("myServiceMethod", Self::my_method),
        ]
    }

    fn my_method(server: &RpcServer, params: &[Value])
        -> Result<Value, RpcException>
    {
        // Implement RPC method
        Ok(json!("result"))
    }
}
```

## Troubleshooting

### Service Not Starting

Check:
1. Feature flag enabled in `Cargo.toml`?
2. Configuration has `enabled = true`?
3. Dependencies available in `ServiceRegistry`?
4. Check logs for startup errors

### Config Validation Errors

Check:
1. TOML syntax correct?
2. Config schema updated?
3. All required fields present?
4. Types match expected values?

### Build Errors

Check:
1. All modules compiled with proper features?
2. Dependencies in `Cargo.toml`?
3. Imports and visibility correct?

## Further Reading

- [Architecture Comparison](./ARCHITECTURE_COMPARISON.md)
- [Deployment Guide](./DEPLOYMENT.md)
- [Operations Guide](./OPERATIONS.md)
- [Neo Node Core Architecture](./ARCHITECTURE.md)
