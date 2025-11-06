# Neo Rust Parity Implementation Roadmap

This document expands on the parity specs for each crate and translates them into concrete implementation milestones. Each phase should include unit/integration tests and reference vectors pulled from the C# implementation.

## Phase 1 – Foundations

1. **neo-base / neo-store preparation**
   - Introduce shared primitive types (UInt160/UInt256, BigInteger helpers, hash wrappers) aligning with C# serialization.
   - Extend `neo-store` with RocksDB (or sled equivalent) column families matching C# storage prefixes.
   - Implement `DataCache`, `StoreCache`, snapshot cloning, and dispose semantics.

2. **neo-crypto enhancements**
   - Deterministic ECDSA (RFC6979) for secp256r1;
   - NEP-2 key wrap/unwrap utilities, Base58Check encoding/decoding;
   - Hash helpers (SHA256, double SHA256, RIPEMD-160, Blake2b, Murmur) and script-hash conversions.

3. **neo-wallet core**
   - Add NEP-6 JSON model, account structures (default/watch-only, multi-sig), signer scopes;
   - Integrate NEP-2 helpers; provide CLI commands for create/import/export.

## Phase 2 – Virtual Machine & Contracts

1. **neo-vm opcode expansion**
   - Implement full opcode set with operand handling, invocation stack, alt stack;
   - Add gas accounting and TRY/CATCH states; integrate interop service table.

2. **StackItem hierarchy & interop services**
   - Introduce `StackItem` trait for primitive and composite types (Array, Map, Struct, Iterator);
   - Implement interop descriptor registry and hook into gas fees.

3. **Contract manifest/runtime**
   - Expand manifest to include ABI, features, trusts, permissions;
   - Add NEF handling, contract state, deployment/update/destroy flows;
   - Integrate call flag enforcement and event notifications.

## Phase 3 – Consensus & Ledger

1. **neo-consensus full dBFT**
   - Add recovery messages, commit signature aggregation, payload caching, timers;
   - Implement persistence via recovery logs and integrate with P2P consensus payloads.

2. **neo-runtime ledger engine**
   - Build blockchain orchestrator to import/persist blocks, manage mempool, trigger native contracts;
   - Implement state storage, GAS/policy/oracle logic, state root handling.

3. **Native contracts suite**
   - Port native contract implementations (NeoToken, GasToken, Policy, Oracle, Ledger, Role, StdLib);
   - Hook `OnPersist`/`PostPersist` flows into runtime.

## Phase 4 – Networking & RPC

1. **neo-p2p protocol parity**
   - Extend message set (headers, filterload/add/clear, mempool, addr2/inv2/tx2, notfound, reject);
   - Implement message framing/compression, handshake state machine, peer manager, task scheduler.

2. **neo-node orchestration**
   - Replace height-tick mock with full P2P loop, block/tx validation, relay, inventory handling;
   - Integrate consensus and runtime and expose telemetry.

3. **RPC server & CLI**
   - Implement JSON-RPC endpoints matching C# (`getblock`, `getrawtransaction`, `invokescript`, `invokecontract`, etc.);
   - Update CLI to consume RPC and provide contract/wallet operations.

## Phase 5 – Integration & Conformance

1. **End-to-end testing**
   - Develop multi-node integration tests (consensus rounds, block propagation);
   - Execute contract deployment/invocation scenarios and compare state with C# node outputs.

2. **Conformance test harness**
   - Build tooling to run Rust and C# nodes in parallel, diffing serialization outputs (opcode traces, consensus payloads, RPC responses).

3. **Performance & benchmarking**
   - Establish benchmarks for VM execution, consensus throughput, RPC latency;
   - Optimize hot paths and ensure gas consumption matches C#.

## Cross-cutting Concerns

- **Documentation & Developer Experience**: Update module docs and onboarding guides as features land, highlighting Rust idioms and differences from C#.
- **CI/CD**: Add build/test jobs per crate; run parity tests using C# golden vectors; enforce formatting/linting.
- **Plugin system**: Evaluate lightweight plugin interfaces once core functionality is stable.

This roadmap is incremental; teams can work in parallel on different phases once prerequisites are satisfied. Tracking issues should reference the corresponding parity spec section and milestone phase.
