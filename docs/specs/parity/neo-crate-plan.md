# Neo Rust Crate Parity Plan

This plan enumerates every root-level `neo-*` crate and records the current parity snapshot, critical gaps, and the next actionable milestone. Update this file whenever work lands so we can trace progress in a document-driven manner.

## How to Work with this Plan

1. **Pick a crate** – read its parity checklist (linked below) plus the roadmap phase.
2. **Update the table** – move the crate's status or next milestone column as soon as work begins or completes.
3. **Implement & test** – run the crate-specific commands from the last column (use `+nightly` when a crate depends on `neo-core`).
4. **Document the outcome** – capture notes, fixtures, and references in the crate's parity doc and in PR descriptions.
5. **Repeat** – advance to the next crate with an 🟥 (blocked) or 🟧 (in progress) status.

_Status legend:_ 🟥 blocked/not started, 🟧 in progress, 🟩 functionally complete (pending polish).

## Crate Status Matrix

| Crate | Parity Spec | Current Snapshot | Critical Gaps | Next Milestone | Tests / Notes |
| --- | --- | --- | --- | --- | --- |
| `neo-base` | [neo-base-parity](./neo-base-parity.md) | 🟧 UInt/Hash primitives split into modules; hashing helpers + codec traits shared across crates. | BigInt/decimal helpers absent; no unified `NeoSerialize/NeoDeserialize` yet; merkle helpers incomplete. | Finish BigInt/decimal codec plus shared serialization derivations. | `cargo test --manifest-path neo-base/Cargo.toml` |
| `neo-cli` | *pending* | 🟥 Rendering split landed, but RPC client modules still stubs; CLI wallet ops limited. | Missing RPC client + integration tests; no contract deployment flows. | Rebuild `neo-cli::rpc::{models,status,wallet}` and add smoke tests for CLI commands. | `cargo +nightly test --manifest-path neo-cli/Cargo.toml` (blocked by `neo-core` edition 2024) |
| `neo-consensus` | [neo-consensus-parity](./neo-consensus-parity.md) | 🟧 DBFT messages/state refactored; snapshot fixtures exist. | Recovery payloads, timers, persistence wiring, and P2P integration still missing. | Implement recovery/timer loop and persistence tests. | `cargo test --manifest-path neo-consensus/Cargo.toml` |
| `neo-contract` | [neo-contract-parity](./neo-contract-parity.md) | 🟧 Manifest/ABI/NEF validation complete; runtime host enforces storage contexts + call flags for storage/notify and Storage.Find/Next now decodes iterator entries (KeysOnly/ValuesOnly/RemovePrefix/Backwards + Deserialize/PickField) into VM stack items. | Call flags still aren’t propagated through contract invocation/deploy flows and native contract plumbing remains TODO. | Thread call flags via `System.Contract.Call` and start wiring native contract hosting. | `cargo +nightly test --manifest-path neo-contract/Cargo.toml` (fails early until `neo-core` drops edition 2024) |
| `neo-core` | *roadmap phase 1/2* | 🟥 Primitive types split but ledger/state orchestration unfinished; crate declares `edition2024` (nightly-only). | Missing block/tx/state orchestrators, policy/oracle state, and stable toolchain support. | Stabilize public API and gate 2024 edition so downstream crates can build on stable. | `cargo +nightly test --manifest-path neo-core/Cargo.toml` |
| `neo-crypto` | [neo-crypto-parity](./neo-crypto-parity.md) | 🟧 Hash suites + secp256r1 RFC6979 vectors + NEP-2 helpers implemented. | Base58Check/Bech32 fixtures limited; NEP-6/CLI integration missing; no BLS. | Add Base58Check + NEP-2 golden vectors and wire crypto helpers into wallet/CLI tests. | `cargo test --manifest-path neo-crypto/Cargo.toml` |
| `neo-node` | *roadmap phase 4* | 🟥 HTTP/RPC split into modules; runtime snapshot helpers exist, but node still mock-only. | No peer manager, P2P relay, runtime persistence, or consensus wiring. | Refactor `Node`/`NodeConfig` into modules and start consuming `neo-p2p` peer events. | `cargo +nightly test --manifest-path neo-node/Cargo.toml --lib` |
| `neo-p2p` | [neo-network-parity](./neo-network-parity.md) | 🟧 Version payload/capabilities, handshake machine, codec framing/compression implemented. | Missing addr2/inv2/tx2/filter* payloads, peer/task manager, and peer scoring. | Implement inventory v2/filter message set plus peer manager loop feeding `neo-node`. | `cargo test --manifest-path neo-p2p/Cargo.toml` |
| `neo-proc-macros` | *roadmap phase 1* | 🟥 Minimal `NeoEncode/NeoDecode` derives only. | Needs manifest/ABI helper derives and StackItem helpers for VM work. | Design macro coverage plan (manifest, NEF, StackItem) and add tests. | `cargo test --manifest-path neo-proc-macros/Cargo.toml` |
| `neo-rpc` | *roadmap phase 4* | 🟥 Request/response structs exist; no running server or transport. | Missing HTTP transport, subscription handling, CLI integration. | Stand up RPC server backed by `neo-node` runtime snapshot (status/block/tx). | `cargo test --manifest-path neo-rpc/Cargo.toml` |
| `neo-runtime` | [neo-runtime-parity](./neo-runtime-parity.md) | 🟥 Runtime facade/mempool scaffolding exist but no state persistence or native contract hooks. | Missing block import pipeline, state DB abstraction, native contract execution. | Implement block import + state persistence (rocksdb or store abstraction) with tests. | `cargo +nightly test --manifest-path neo-runtime/Cargo.toml` |
| `neo-store` | *roadmap phase 1* | 🟧 Memory + sled stores with ordered prefix scans available. | Needs RocksDB backend, cache layering, snapshot cloning/disposal semantics. | Introduce RocksDB-backed store with cache layers & snapshot tests. | `cargo test --manifest-path neo-store/Cargo.toml` |
| `neo-vm` | [neo-vm-parity](./neo-vm-parity.md) | 🟧 Opcode surface expanded (stack ops, iterators, storage syscalls, call flag query) with initial StackItem scaffolding + VmValue conversions. | StackItems aren’t used by the execution engine or BinarySerializer yet; gas accounting, TRY/CATCH, and iterator registries remain unimplemented. | Integrate StackItems into execution/runtime serialization and start wiring gas accounting. | `cargo test --manifest-path neo-vm/Cargo.toml` |
| `neo-wallet` | [neo-wallet-parity](./neo-wallet-parity.md) | 🟥 Wallet/account models split but NEP-6 operations and signer scopes incomplete. | Missing NEP-6 persistence, NEP-2 CLI workflows, and RPC integration. | Implement NEP-6 import/export + signer scope handling, and add wallet CLI tests. | `cargo test --manifest-path neo-wallet/Cargo.toml` |

## Immediate Focus Queue

1. **neo-contract** – implement `StorageFind` deserialization/pick semantics and propagate call flags through contract invocation/deploy paths.
2. **neo-vm** – add StackItem hierarchy + gas accounting so iterator/native syscalls can return real stack items.
3. **neo-node / neo-p2p** – refactor `Node`/`NodeConfig`, introduce peer manager from `neo-p2p`, and start relaying inventory.
4. **neo-runtime** – design state DB abstraction + block import pipeline to back the node/RPC layers.
