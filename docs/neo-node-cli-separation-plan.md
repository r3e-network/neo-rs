# Neo Node / CLI Separation Plan

## Overview

Split the current `neo-cli` crate into two separate components:
1. **neo-node**: Standalone daemon that runs the Neo blockchain node with RPC server
2. **neo-cli**: Command-line client that communicates with neo-node via RPC

## Current Architecture

- `crates/cli/` - Monolithic application that embeds node + interactive console
- `crates/plugins/rpc_server/` - RPC server plugin (already exists)
- `crates/rpc_client/` - RPC client library (already exists)

## Target Architecture

```
┌─────────────┐         JSON-RPC          ┌─────────────┐
│   neo-cli   │ ◄─────────────────────────► │  neo-node   │
│  (client)   │      HTTP/HTTPS            │  (daemon)   │
└─────────────┘                            └─────────────┘
     │                                           │
     │                                           │
     ▼                                           ▼
┌─────────────┐                            ┌─────────────┐
│ rpc_client  │                            │  NeoSystem  │
│   crate     │                            │  + Plugins  │
└─────────────┘                            └─────────────┘
```

## Implementation Steps

### Phase 1: Create neo-node crate

1. Create `crates/node/` directory structure
2. Create `Cargo.toml` with dependencies from cli
3. Move node startup logic from cli/main.rs to node/main.rs
4. Keep RPC server plugin integration
5. Remove interactive console code

### Phase 2: Refactor neo-cli to RPC client

1. Remove NeoSystem dependency from neo-cli
2. Add neo-rpc-client dependency
3. Convert all commands to use RPC calls:
   - `neo-cli wallet open <path>` → RPC call
   - `neo-cli show state` → `getblockcount`, `getversion`
   - `neo-cli send <asset> <to> <amount>` → `sendrawtransaction`
   - etc.
4. Use clap subcommands for CLI structure

### Phase 3: Update workspace

1. Add `crates/node` to workspace members
2. Update dependencies
3. Ensure both binaries can be built

## neo-cli Command Structure

```
neo-cli [OPTIONS] <COMMAND>

OPTIONS:
  --rpc-url <URL>       RPC server URL (default: http://localhost:10332)
  --rpc-user <USER>     RPC basic auth username
  --rpc-pass <PASS>     RPC basic auth password
  -v, --verbose         Verbose output
  -h, --help            Print help

COMMANDS:
  # Node info
  version               Get node version
  state                 Show node state (block height, connections)
  peers                 Show connected peers
  mempool               Show memory pool

  # Blockchain queries
  block <index|hash>    Get block by index or hash
  tx <hash>             Get transaction by hash
  contract <hash>       Get contract state

  # Wallet operations (requires wallet file)
  wallet open <path>    Open wallet (prompts for password)
  wallet create <path>  Create new wallet
  wallet list           List addresses in wallet
  wallet balance        Show wallet balances
  wallet export-key     Export private keys

  # Transactions
  send <asset> <to> <amount> [--from <addr>]
  transfer <token> <to> <amount> [--from <addr>]
  invoke <hash> <method> [params...]

  # Voting
  vote <account> <pubkey>
  candidates            List candidates
  committee             Show committee members

  # Tools
  parse <value>         Parse address/hash/script
```

## Key Files to Create/Modify

### New Files
- `crates/node/Cargo.toml`
- `crates/node/src/main.rs`
- `crates/node/src/config.rs` (copy from cli)

### Modified Files
- `Cargo.toml` (workspace)
- `crates/cli/Cargo.toml`
- `crates/cli/src/main.rs` (complete rewrite)
- `crates/cli/src/commands/*.rs` (convert to RPC)

## RPC Methods Mapping

| CLI Command | RPC Method(s) |
|-------------|---------------|
| show state | getblockcount, getversion |
| show block | getblock |
| show tx | getrawtransaction |
| show contract | getcontractstate |
| show pool | getrawmempool |
| show node | getpeers |
| send | sendrawtransaction |
| invoke | invokefunction |
| vote | invokefunction (NEO contract) |
| balance | getnep17balances |
