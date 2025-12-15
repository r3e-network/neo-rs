# Rust ↔ C# Parity Map

This document explains how the Rust full‑node stack in this repository (`neo-rs`) maps to the official Neo N3 C# node (`neo_csharp/`). The goal is to make it easy to reason about correctness and to port fixes/features by following a predictable crate/module/file relationship.

**Reference versions**
- Rust workspace: `neo-rs` v0.7.x (this repo).
- C# reference: Neo N3 v3.8.2 (`neo_csharp/` is pinned to commit `ede620e` per `README.md`).

## Naming and Structure Conventions

### File‑level parity rule
Unless explicitly noted in “Divergences”, Rust source files intentionally mirror C# file names:

```
snake_case.rs  ↔  PascalCase.cs
```

Examples:
- `neo-vm/src/execution_engine.rs` ↔ `neo_csharp/src/Neo.VM/ExecutionEngine.cs`
- `neo-core/src/ledger/blockchain.rs` ↔ `neo_csharp/src/Neo/Ledger/Blockchain.cs`
- `neo-plugins/src/dbft_plugin/consensus/consensus_context.rs`
  ↔ `neo_csharp/src/Plugins/DBFTPlugin/Consensus/ConsensusContext.cs`

### Directory / namespace parity rule
Rust module trees are laid out to match C# namespaces:

```
neo-*/src/<module>/...  ↔  neo_csharp/src/<Project>/<Namespace>/...
```

Examples:
- `neo-core/src/network/p2p/...` ↔ `neo_csharp/src/Neo/Network/P2P/...`
- `neo-core/src/smart_contract/native/...` ↔ `neo_csharp/src/Neo/SmartContract/Native/...`
- `neo-plugins/src/rpc_server/...` ↔ `neo_csharp/src/Plugins/RpcServer/...`

## Crate‑by‑Crate Parity

### Foundation Layer

#### `neo-primitives`
**C# project:** `neo_csharp/src/Neo/`

Primary type parity:
- `neo-primitives/src/uint160.rs` ↔ `neo_csharp/src/Neo/UInt160.cs`
- `neo-primitives/src/uint256.rs` ↔ `neo_csharp/src/Neo/UInt256.cs`
- `neo-primitives/src/big_decimal.rs` ↔ `neo_csharp/src/Neo/BigDecimal.cs`
- `neo-primitives/src/hardfork.rs` ↔ `neo_csharp/src/Neo/Hardfork.cs`
- `neo-primitives/src/constants.rs` ↔ constants in `neo_csharp/src/Neo/` and protocol constants under `Neo/`

Smart‑contract shared enums:
- `neo-primitives/src/contract_parameter_type.rs`
  ↔ `neo_csharp/src/Neo/SmartContract/ContractParameterType.cs`

#### `neo-crypto`
**C# projects:**
- Core crypto: `neo_csharp/src/Neo/Cryptography/`
- BLS12‑381: `neo_csharp/src/Neo.Cryptography.BLS12_381/`
- MPT trie crypto: `neo_csharp/src/Neo.Cryptography.MPTTrie/`

Key file parity:
- `neo-crypto/src/hash.rs` ↔ `neo_csharp/src/Neo/Cryptography/Crypto.cs`
- `neo-crypto/src/ecc.rs` ↔ `neo_csharp/src/Neo/Cryptography/ECC/*.cs`
- `neo-crypto/src/base58.rs` ↔ `neo_csharp/src/Neo/Cryptography/Base58.cs`
- BLS helpers in `neo-core/src/cryptography/crypto_utils.rs`
  ↔ `neo_csharp/src/Neo.Cryptography.BLS12_381/*.cs`

#### `neo-storage`
**C# namespace:** `neo_csharp/src/Neo/Persistence/`

Parity examples:
- `neo-storage/src/store.rs` ↔ `neo_csharp/src/Neo/Persistence/IStore.cs`
- `neo-storage/src/snapshot.rs` ↔ `neo_csharp/src/Neo/Persistence/StoreView.cs`

#### `neo-io`
**C# project:** `neo_csharp/src/Neo.IO/` and `neo_csharp/src/Neo/IO/`

Parity examples:
- `neo-io/src/binary_reader.rs` ↔ `Neo.IO/BinaryReader.cs`
- `neo-io/src/binary_writer.rs` ↔ `Neo.IO/BinaryWriter.cs`
- `neo-io/src/caching/*` ↔ `Neo.IO/Caching/*`

#### `neo-json`
**C# project:** `neo_csharp/src/Neo.Json/`

Parity examples:
- `neo-json/src/j_token.rs` ↔ `Neo.Json/JToken.cs`
- `neo-json/src/j_object.rs` ↔ `Neo.Json/JObject.cs`
- `neo-json/src/j_array.rs` ↔ `Neo.Json/JArray.cs`
- `neo-json/src/j_path*.rs` ↔ `Neo.Json/JPath*.cs`

### Core Layer

#### `neo-vm`
**C# project:** `neo_csharp/src/Neo.VM/`

Top‑level parity:
- `neo-vm/src/execution_engine.rs` ↔ `Neo.VM/ExecutionEngine.cs`
- `neo-vm/src/execution_context.rs` ↔ `Neo.VM/ExecutionContext.cs`
- `neo-vm/src/evaluation_stack.rs` ↔ `Neo.VM/EvaluationStack.cs`
- `neo-vm/src/op_code/*` ↔ `Neo.VM/OpCode.cs` + opcode metadata
- `neo-vm/src/jump_table/*` ↔ `Neo.VM/JumpTable/*`
- `neo-vm/src/stack_item/*` ↔ `Neo.VM/Types/*`
- `neo-vm/src/interop_service.rs` ↔ `Neo.VM/InteropService.cs`

#### `neo-contract`
**C# namespace:** `neo_csharp/src/Neo/SmartContract/`

Parity (all files are 1:1 snake ↔ Pascal case):
- `trigger_type.rs` ↔ `TriggerType.cs`
- `find_options.rs` ↔ `FindOptions.cs`
- `method_token.rs` ↔ `MethodToken.cs`
- `storage_context.rs` ↔ `StorageContext.cs`
- `role.rs` ↔ `Native/Role.cs`
- `contract_basic_method.rs` ↔ `ContractBasicMethod.cs`

#### `neo-p2p`
**C# namespaces:** 
- `neo_csharp/src/Neo/Network/P2P/`
- witness rule types in `neo_csharp/src/Neo/Network/P2P/Payloads/`

Parity examples:
- `message_command.rs` ↔ `Network/P2P/MessageCommand.cs`
- `inventory_type.rs` ↔ `Network/P2P/InventoryType.cs`
- `node_capability_type.rs` ↔ `Network/P2P/Capabilities/NodeCapabilityType.cs`
- `witness_condition_type.rs`, `witness_rule_action.rs`
  ↔ `Network/P2P/Payloads/WitnessConditionType.cs`, `WitnessRuleAction.cs`

#### `neo-core`
**C# project:** `neo_csharp/src/Neo/`

Rust `neo-core` is the bulk of protocol logic and mirrors the same directory layout as C# `Neo`:

| Rust module | C# namespace / folder |
|-------------|------------------------|
| `builders/` | `Neo/Builders/` |
| `cryptography/` | `Neo/Cryptography/` |
| `extensions/` | `Neo/Extensions/` |
| `ledger/` | `Neo/Ledger/` |
| `network/` | `Neo/Network/` |
| `persistence/` | `Neo/Persistence/` |
| `protocol_settings.rs` | `Neo/ProtocolSettings.cs` |
| `neo_system/` | `Neo/NeoSystem*.cs` |
| `smart_contract/` | `Neo/SmartContract/` |
| `services/` | `Neo/Services/` |
| `state_service/` | `Neo/StateService/` |
| `wallets/` | `Neo/Wallets/` |

Within each directory, file names track C# 1:1 (snake_case ↔ PascalCase). Example mappings:
- `neo-core/src/ledger/block.rs` ↔ `Neo/Ledger/Block.cs`
- `neo-core/src/ledger/transaction.rs` ↔ `Neo/Ledger/Transaction.cs`
- `neo-core/src/network/p2p/messages.rs` ↔ `Neo/Network/P2P/Message.cs`
- `neo-core/src/smart_contract/application_engine.rs` ↔ `Neo/SmartContract/ApplicationEngine.cs`
- `neo-core/src/witness.rs` ↔ `Neo/Network/P2P/Payloads/Witness.cs`

#### `neo-rpc`
**C# namespaces:** 
- server errors: `neo_csharp/src/Plugins/RpcServer/`
- client errors: `neo_csharp/src/RpcClient/`

Parity examples:
- `neo-rpc/src/error_code.rs` ↔ `Plugins/RpcServer/RpcErrorCode.cs`
- `neo-rpc/src/error.rs` ↔ `RpcClient/RpcException.cs`

#### `neo-consensus`
**C# project:** `neo_csharp/src/Plugins/DBFTPlugin/Types/`

Parity is type‑level:
- `neo-consensus/src/message_type.rs` ↔ `Types/ConsensusMessageType.cs`
- `neo-consensus/src/change_view_reason.rs` ↔ `Types/ChangeViewReason.cs`
- `neo-consensus/src/error.rs` ↔ `Types/ConsensusError.cs`

The dBFT runtime itself lives in `neo-plugins/dbft_plugin` (see below).

### Infrastructure Layer

#### `neo-plugins`
**C# project:** `neo_csharp/src/Plugins/`

Plugin parity map:
- `neo-plugins/src/dbft_plugin/` ↔ `Plugins/DBFTPlugin/`
- `neo-plugins/src/rpc_server/` ↔ `Plugins/RpcServer/`
- `neo-plugins/src/application_logs/` ↔ `Plugins/ApplicationLogs/`
- `neo-plugins/src/rocksdb_store/` ↔ `Plugins/RocksDBStore/`
- `neo-plugins/src/sqlite_wallet/` ↔ `Plugins/SQLiteWallet/`
- `neo-plugins/src/tokens_tracker/` ↔ `Plugins/TokensTracker/`

#### `neo-rpc-client`
**C# project:** `neo_csharp/src/RpcClient/` and RPC helper APIs under `Plugins/*`

Rust client APIs mirror the C# RpcClient surface:
- `neo-rpc-client/src/rpc_client.rs` ↔ `RpcClient/RpcClient.cs`
- `neo-rpc-client/src/nep17_api.rs` ↔ `RpcClient/Api/Nep17API.cs`
- `neo-rpc-client/src/policy_api.rs` ↔ `RpcClient/Api/PolicyAPI.cs`

#### `neo-services`
Rust‑only service trait crate for typed DI over `NeoSystem`.  
There is no direct C# project equivalent; this abstracts C#’s runtime service locator patterns.

#### `neo-akka`
Rust actor runtime inspired by Akka.NET.  
No direct C# file parity; C# Neo uses Akka.NET and/or internal actor patterns, while Rust provides a lightweight compatible façade.

#### `neo-tee`
Rust‑only Trusted Execution Environment integration.  
No C# parity; this is an extension specific to `neo-rs`.

### Application Layer

#### `neo-node`
**C# project:** `neo_csharp/src/Neo.ConsoleService/`

Parity examples:
- `neo-node/src/main.rs` ↔ `Neo.ConsoleService/Program.cs`
- `neo-node/src/config.rs` ↔ `Neo.ConsoleService/Settings.cs` + config loading helpers

#### `neo-cli`
**C# project:** `neo_csharp/src/Neo.CLI/`

Parity examples:
- `neo-cli/src/main.rs` ↔ `Neo.CLI/Program.cs`

## Full File-by-File Tables

The exhaustive, auto-generated parity tables (every Rust `src/**/*.rs` file mapped to its C# counterpart when present) live in:

- `docs/PARITY_FILES.md`

These tables are produced by matching file names within each crate to the corresponding C# project roots described above. For ambiguous names (e.g., multiple `Helper.cs` files), all matches are listed and the crate‑level mapping should be used to pick the correct semantic counterpart.
- command modules under `neo-cli/src/commands/`
  ↔ `Neo.CLI/Commands/`

## Known Divergences / Rust‑Specific Extensions

- **TokensTracker**: `neo-plugins/src/tokens_tracker/` is a functional port of the C# TokensTracker plugin and exposes the NEP-11/NEP-17 balance/transfer JSON-RPC endpoints (`getnep17balances`, `getnep17transfers`, `getnep11balances`, `getnep11transfers`, `getnep11properties`).
- **TEE support**: `neo-tee` and the `--tee` mode in `neo-node` have no C# analog.
- **TLS for RPC**: C# RpcServer supports TLS directly. Rust RpcServer currently refuses TLS config and requires an external terminator.
- **Removed experimental plugins**: C# plugins such as SignClient, StorageDumper, Oracle, StateService, and LevelDBStore are not ported in Rust yet.
- **Actor system implementation**: C# uses Akka.NET directly; Rust uses the in‑repo `neo-akka` crate to provide similar semantics.

For any porting work, start from the crate/project mapping above, then follow the file‑level parity rule to locate the corresponding implementation.
