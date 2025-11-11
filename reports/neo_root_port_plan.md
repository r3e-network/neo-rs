# Neo Root-Crate Parity Roadmap

This roadmap tracks the work required to make the root `neo-*` crates feature complete and behaviorally aligned with the canonical C# implementation located under `neo_csharp/`. Each section highlights the current Rust status, the matching subsystem in C#, and concrete tasks to close the gap.

## Execution Strategy

1. **Stabilize primitives first** – Finish `neo-core` so every other crate (rpc, cli, contract) can rely on canonical serialization, hashing, and policy lookups. Ship fixtures/tests together with each feature.
2. **Layer smart-contract runtime** – Once the primitives are stable, finish `neo-contract` (NEF/manifest, native contracts, ApplicationEngine). This unblocks RPC `invoke*` and CLI deployment tooling.
3. **Expose capabilities via RPC** – Build `neo-rpc` on top of the stable runtime/core. Start with blockchain/query methods, then mempool/wallet, finally deployment/invocation.
4. **Drive tooling through RPC** – Rebuild `neo-cli` as an RPC-first console mirroring Neo C# commands. Retire legacy REST handlers along the way.
5. **Parity validation** – For every milestone, add integration tests that replay fixtures captured from `neo_csharp` (serialized blocks, RPC responses, CLI transcripts).

Dependencies: `neo-core` → `neo-contract` → (`neo-rpc`, `neo-cli`). Workstreams can overlap, but each downstream crate should only consume released APIs from its prerequisites.

## neo-core

| Capability | Rust status | Neo C# reference | Required work |
| --- | --- | --- | --- |
| Protocol settings coverage | `ProtocolSettings` exposes network magic, address version, committee, and scrypt defaults. | `neo_csharp/src/Neo/ProtocolSettings.cs` | Add `MaxValidUntilBlockIncrement`, `Hardforks`, `InitialGasDistribution`, seed list overrides, and validators count overrides. Provide JSON/TOML loaders plus validation tests so every crate shares identical settings. |
| Contract/NEF types | NEF + manifest support recently landed but lacks contract state, permissions, trusts, and ABI event helpers. | `neo_csharp/src/Neo/SmartContract/NefFile.cs`, `ContractState.cs`, `Manifest/ContractManifest.cs` | Finish manifest JSON/binary parity (`features`, `supportedstandards`, `trusts`, `extra`), implement `ContractState`, and port method/event descriptors. Capture fixtures from the C# node. |
| Transaction & attribute validation | Binary encoding exists, but verifier helpers, policy lookups, and witness hashing are missing. | `neo_csharp/src/Neo/Network/P2P/Payloads/Transaction.cs` + `TransactionAttribute*.cs` | Port `GetScriptHashesForVerifying`, attribute-specific `Verify`/fee logic, Policy/GAS contract hooks, and witness evaluation helpers so RPC/CLI can reuse them. |
| Block/header utilities | Serialization is in place without Merkle root helpers, trimmed block conversions, or hash caching semantics. | `neo_csharp/src/Neo/Network/P2P/Payloads/Block.cs` | Implement `block/merkle.rs`, state-root flags, and trimmed block helpers. Cache unsigned data hashes for RPC throughput. |

## neo-contract

| Capability | Rust status | Neo C# reference | Required work |
| --- | --- | --- | --- |
| Application engine | `ExecutionContext` + syscall dispatcher exist, but there is no ApplicationEngine orchestrator, interop service, or stack abstractions. | `neo_csharp/src/Neo/SmartContract/ApplicationEngine.cs` | Introduce ApplicationEngine with evaluation stack, invocation stack, `CallFlags`, and interop routing. Wire into `neo-vm` once available. |
| Native contracts | Registry exists with placeholders; Ledger/GAS/Policy/Oracle/etc. are missing. | `neo_csharp/src/Neo/SmartContract/Native/*.cs` | Port manifests + method handlers for each native contract. Provide storage schemas backed by `neo-store` and hook gas accounting identical to C#. |
| Manifest & NEF lifecycle | NEF + manifest parsing implemented, but deployment helpers, contract state, and permission evaluation are incomplete. | `neo_csharp/src/Neo/SmartContract/ContractState.cs`, `ContractManifest.cs` | Add contract state persistence, permission validation helpers, `ContractGroup` verification, and JSON converters used by RPC/CLI. |
| Witness evaluation | Witness scopes/rules implemented in `neo-core`, but runtime doesn’t yet load real signer/contract context. | `neo_csharp/src/Neo/SmartContract/ApplicationEngine.Runtime.cs` | Add helpers to populate signers, current/calling script hashes, and manifest groups from transactions so syscalls like `CheckWitness` match C# behavior during actual execution. |

## neo-rpc

| Capability | Rust status | Neo C# reference | Required work |
| --- | --- | --- | --- |
| Core blockchain RPC | `getblock*`, `getstatus`, and wallet helper methods exist; block/tx JSON formats lag C# and advanced filters are missing. | `neo_csharp/src/Neo.Plugins/RpcServer/RpcServer.cs` | Align block/tx JSON fields, expose `getblockheader`, `getstateheight`, `getrawtransaction`, etc. Ensure height/hash lookup semantics match the plugin. |
| Mempool + submission | No transaction submission, fee estimation, or mempool queries. | `RpcServer.RestController` + `RpcServer/TransactionHandlers` | Add `sendrawtransaction`, `getrawmempool`, and fee estimates using `neo-runtime`’s pool. Hook verification helpers from `neo-core`. |
| Smart contract invocation | `invoke*`/`testinvoke*` endpoints absent; no ApplicationEngine integration. | `neo_csharp/src/Neo.Plugins/RpcServer/Scripts.cs` | Once `neo-contract` exposes ApplicationEngine, wire up `invokefunction`, `invokescript`, `invokecontractverify`, returning stack JSON identical to C#. |
| Wallet / consensus queries | Wallet RPCs exist but rely on REST leftovers; consensus status mostly covered. | `RpcServer/WalletHandlers.cs` | Finish migrating REST endpoints to RPC (wallet detail/export/update complete but remove REST duplicates). Add `getpeers`, `getconnectioncount`, and `getcommittee` using `neo-p2p`/consensus state. |

## neo-cli

| Capability | Rust status | Neo C# reference | Required work |
| --- | --- | --- | --- |
| Command surface | Basic wallet commands call the new RPC endpoints; most C# commands (contract deploy/invoke, node mgmt, policy updates) are missing. | `neo_csharp/src/Neo.CLI/Program.cs` | Mirror the C# command tree (`show state`, `list address`, `send`, `invokefunction`, `deploy`, `policy gas`, etc.) using RPC-first flows. |
| Wallet management | Import/export/list flows ported; signer metadata editing and offline signing not available. | `Neo.CLI/Wallet/Commands.cs` | Add commands for default account management, signer scope edits, NEP-2 conversions, and offline transaction signing (delegate to `neo-core` serializers). |
| Contract lifecycle | No deployment/invocation tooling yet. | `Neo.CLI/SmartContractService.cs` | Implement `contract deploy`, `contract invoke`, and `contract update` commands that compile NEF/manifest, push them via RPC, and display VM result/stack. |
| Diagnostics & scripting | Missing `show state`, `show pool`, `relay`, `dumpprivkey`, and interactive console features. | `Neo.CLI` interactive shell | Add status commands backed by `neo-rpc`, integrate `script` command to build scripts with `neo-core::script::Builder`, and provide JSON/pretty output toggles. |

## Validation & Tooling

- **Fixtures**: Capture canonical inputs/outputs from `neo_csharp` (blocks, transactions, RPC payloads, CLI transcripts) and store them under `reports/fixtures`. Regressions should diff against these snapshots.
- **End-to-end smoke test**: Once `neo-contract` + `neo-rpc` + `neo-cli` milestones land, build a scenario (deploy contract → invoke via CLI → query via RPC) to ensure the stack interops correctly.

This document should be revisited after each milestone to incorporate newly discovered gaps or upstream C# changes.
