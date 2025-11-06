# Neo Networking & RPC Parity Checklist (Rust vs. C#)

## Summary

The Rust stack (`neo-p2p`, `neo-node`, `neo-cli`) currently stubs out networking: only a handful of P2P messages are defined, there is no message framing, checksum, peer management, or RPC surface equivalent to the C# node (`Neo.Network.P2P`, `Neo.Network.P2P.Payloads`, `Neo.IO.Caching`, `Neo.Network.RPC`). This document enumerates the components required for parity and outlines an implementation roadmap.

## Components to Map

1. **Message Transport (C# `Message.cs`)**
   - Message flags, compression (LZ4), payload max size checks.
   - Command enum covering `version`, `verack`, `ping`, `pong`, `addr`, `getaddr`, `inv`, `getdata`, `block`, `headers`, `filterload`, `filteradd`, `merkleblock`, `notfound`, `reject`, `consensus`, `extensible`, `addr2`, `inv2`, `tx2`, `headers2`, `ping2`, `alert` (where applicable).
   - Message serialization (var-bytes, checksums) and reflection-based payload creation.

2. **Payload Types (C# `Neo.Network.P2P.Payloads`)**
   - `VersionPayload`, `PingPayload`, `AddressPayload`, `HeadersPayload`, `InventoryPayload`, `FilterLoad`, `FilterAdd`, `MerkleBlockPayload`, `ExtensiblePayload`, `ConsensusPayload`, `Transaction`, `Block`, `Witness`, `ClaimTransaction`, `OracleResponse`, etc.
   - `Inventory` interface behaviour (`GetHash`, `Size`, `GetMessageCommand`).
   - `FilterLoad` bloom filters, `MerkleBlock` partial tree logic.

3. **Peer Management (`LocalNode`, `RemoteNode`, `Connection`)**
   - Handshake state machine (nonce, services, user agent, timestamp).
   - Peer list, address propagation (`addr`/`addr2`), peer scoring/banning.
   - Task scheduling (`TaskManager`, `TaskSession`) for block/tx requests and inventory relay.
   - Bandwidth throttling, message queues, handshake timeouts.
   - Capability negotiation (full node vs state root, etc.).

4. **Mempool and Relay (`Blockchain` integration)**
   - Relay inventory to peers, re-verification, `RelayResult` messages.
   - `BloomFilter`, `FilterAdd`, `FilterClear`.

5. **Extensible/Consensus Payloads**
   - Handling of `MessageCommand.Extensible` and `MessageCommand.Consensus`, tying into `neo-consensus`.

6. **RPC (`Neo.Network.RPC`, `Neo.Modules`)**
   - JSON-RPC server: endpoints like `getblock`, `getrawtransaction`, `invokecontract`, `invokescript`, `getstate`, `getversion`, `validateaddress`, `getscripthash`, etc.
   - CLI integration (C# `neo-cli` commands) for contract deployment, invocation, wallet management, policy/oracle operations.
   - REST/gRPC (if targeted) parity decisions.

7. **Configuration (`Neo.IO.Caching`, `Neo.Settings`)**
   - Node configuration (P2P port, RPC settings, seed nodes, plugin paths, WS competition, bootstrap).
   - Logging, diagnostics, plugin load mechanism.

## Current Rust Gaps

- `neo-p2p/src/message.rs`: only defines commands for `Version`, `Verack`, `Ping`, `Pong`, `GetAddr`, `Address`, `Inventory`, `GetData`, `Block`, `Transaction`. Missing majority of message types and payload variants.
- No message framing or compression (C# uses flags, `CompressLz4`, checksums).
- No handshake state machine beyond simple codec; no peer manager, no task scheduler.
- `neo-node` does not implement P2P loops; it fakes consensus progress with timers.
- No inventory relay, mempool re-verification, or block request scheduling.
- No RPC server in Rust; CLI only prints status/consensus metrics.
- No integration with consensus for consensus/relay payloads.

## Implementation Plan

1. **Message Infrastructure**
   - Recreate `MessageFlags`, `MessageCommand`, message serialization (var-bytes, checksums, optional compression).
   - Implement LZ4 compression (or compatible library) and checksum verification.
   - Extend `Message` enum to include all N3 commands with appropriate payloads.

2. **Payload Definitions**
   - Port payload structures from C# (Version, Ping, Address, Inventory variants, Headers, MerkleBlock, Filter load/add/clear, NotFound, Reject, Extensible, Consensus).
   - Ensure serialization matches C#; add tests comparing to golden payloads.
   - Implement `Inventory` trait for Block/Transaction/Consensus payloads.

3. **Peer Runner**
   - Build `Peer` state machine handling handshake, version negotiation, verack, ping/pong keep-alive, network services.
   - Maintain peer registry similar to `LocalNode`: address manager, connection limits, drop policies, ban thresholds.
   - Implement task manager for block/tx/inventory requests, scheduling, and timeouts.
   - Integrate with `neo-runtime`/mempool for relay decisions.

4. **Consensus & Extensible Integration**
   - Support `MessageCommand.Consensus` and `Extensible` to pass through to consensus engine.
   - Relay consensus payloads to peers with appropriate validation.

5. **RPC Surface**
   - Implement a JSON-RPC server (Axum/hyper) mirroring C# RPC endpoints.
   - Expose ledger, mempool, consensus, contract invocation, wallet, oracle, policy APIs.
   - Ensure CLI interacts via RPC rather than direct internal calls; replicate C# `neo-cli` commands.

6. **Configuration & Plugins**
   - Provide Rust equivalents of node settings: P2P port, RPC endpoints, seed nodes, logging, plugin directories.
   - Design plugin interface if plugin support is desired (can be deferred).

7. **Testing & Validation**
   - Create integration tests that mimic handshake sequences, message round-trips, and block relay using C# nodes as reference.
   - Add fuzz tests for message parsing to ensure robustness.
   - Establish metrics/logging for P2P and RPC operations.

## Milestones

1. Message & payload parity (serialization, compression, tests).
2. Peer manager and handshake implementation with basic block/tx relay.
3. RPC server parity with essential endpoints, CLI updates.
4. Consensus payload integration and network-driven block propagation.
5. Advanced features: bloom filters, merkle block support, notfound/reject handling, plugin integration.

## Next Steps

- Produce parity documents for wallet/contract/crypto modules to complete specification coverage.
- Agree on P2P architecture (Tokio tasks, actor model, or async channels).
- Collect C# message capture samples to use as golden vectors for testing Rust serialization/deserialization.
