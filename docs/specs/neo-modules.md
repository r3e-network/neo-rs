# Neo N3 Rust Modules – Design Snapshot

This document captures the high–level intent and API surface for the standalone crates that will make up the public Neo N3 Rust SDK. Each section focuses on the responsibilities of the crate, the main types we expose, and the dependencies we rely on. Implementation should remain aligned with the goals described here.

## neo-base
- **Role**: Provide zero-allocation friendly primitives shared across networking, storage, consensus, and contract execution layers.
- **Key areas**:
- `Bytes` wrapper with deterministic serialization, display helpers, and serde support.
- Binary codec traits (`NeoEncode`/`NeoDecode`) with varint framing rules identical to the C# reference implementation.
- Optional `derive` feature that re-exports procedural macros for the codec traits to minimise boilerplate.
  - Hash helpers for SHA-256, double SHA-256, RIPEMD-160, and Keccak-256 plus typed hash wrappers (`Hash256`, `Hash160`).
  - Minimal Merkle tree builder returning root + proof paths.
  - Time utilities gated behind `std` for wall clock interactions, while core crate remains `no_std`.
- **Dependencies**: `bytes`, `sha2`, `ripemd`, `bs58`, `base64`, `hex`, `serde` (alloc only). Enable optional `chrono` when `std`.

## neo-crypto
- **Role**: Thin ergonomic layer around the cryptographic primitives used by the Neo protocol.
- **Key areas**:
  - Constant-time `SecretKey` wrapper with `Zeroize` support.
  - `DeriveScryptKey` helper for wallet password stretching.
  - P-256 ECDSA signing and verification building on `neo-base` encodings.
  - AES-256-ECB primitives for legacy NEP-2 key wrapping.
  - HMAC-SHA256 helper for message MACs.
  - Public/private key structs that can serialize via `NeoEncode`/`NeoDecode`.
- **Dependencies**: `p256`, `aes`, `hmac`, `sha2`, `scrypt`, `subtle`, `zeroize`, `thiserror`. The crate is `no_std` compatible.

## neo-store
- **Role**: Abstract the key-value storage layer used by the node (header store, state store, log store).
- **Key areas**:
  - `StoreBackend` trait with typed column families and snapshot semantics.
  - `MemoryStore` reference implementation for tests and deterministic simulations.
  - Persistent `SledStore` backend used by `neo-node` for consensus snapshot durability.
  - High-level helpers for block persistence (batched writes, iterators).
  - Feature flag to compile RocksDB integration at a later stage.
  - Typed column definitions (`Headers`, `Blocks`) and fixtures under `neo-store/fixtures/` demonstrate how JSON configuration can hydrate the store.
- **Dependencies (initial)**: `neo-base` for encoding, `dashmap` and `parking_lot` for concurrency-safe in-memory backend, `thiserror` for error reporting.

## neo-p2p
- **Role**: Encode/decode the P2P wire protocol, manage peer state, and provide async-friendly message channels.
- **Key areas**:
  - Message enum mirroring the N3 wire protocol with versioning support.
  - Handshake flow (`Version`, `Verack`, `Ping`, `Pong`) and inventory broadcasting.
  - Peer management traits (dialer, listener, ban scoring) without committing to a runtime.
  - Deterministic serialization via `neo-base` codec traits.
- **Dependencies (initial)**: `neo-base`, `bytes`, `tokio-util` (framing), `futures`, `thiserror`, `tracing`.

## neo-proc-macros
- **Role**: Provide derive macros that remove boilerplate when implementing the binary codec traits.
- **Key areas**:
  - `NeoEncode`/`NeoDecode` derives supporting enums and structs.
  - Attribute to control varint length, byte order, and version gating.
  - Re-exported by `neo-base` behind a `derive` feature.
- **Dependencies**: `proc-macro2`, `quote`, `syn`, `darling` for attribute parsing.

## neo-consensus
- **Role**: Capture the core dBFT logic (message types, validator registry, consensus engine) required to drive block production.
- **Key areas**:
  - Strongly typed consensus messages with signature helpers and digest calculation.
- `ConsensusState` that tracks validator participation and proposal integrity with quorum checks.
- Enforces message ordering so a validator must emit a `PrepareResponse` before its `Commit` is accepted.
- `DbftEngine` wrapper that verifies signatures and advances the state machine, plus replay helpers for recovery.
- View-change quorum handling that validates incoming requests and bumps the active view once thresholds are met.
- Snapshot/export surface (`SnapshotState`) with restoration helpers so consensus participation survives node restarts.
- Primary selection helpers (`ValidatorSet::primary_id`) enforcing that prepare requests originate from the designated leader for a given height/view.
- Height transition API that resets view/proposal tracking when advancing to the next block.
- Participation, tally, and missing-validator accessors used by node telemetry for live consensus reporting.
- Quorum decisions expose the validators that still need to vote, using message-specific expectations (e.g. only the primary is marked missing for `PrepareRequest` and commit/change-view gaps are only surfaced once the stage is active) so callers can react to partial quorums without over-reporting.
- `ConsensusState` pre-seeds the `PrepareRequest` expectation with the current primary and refreshes it after view or height transitions so telemetry immediately highlights when the designated leader has not yet broadcast.
- **Dependencies**: `neo-base` for hashing/encoding, `neo-crypto` for secp256r1 signing and verification, `hashbrown` for efficient message indexing.

## neo-contract
- **Role**: Provide smart-contract metadata handling, native contract registry, and execution-time utilities shared by the VM.
- **Key areas**:
  - Manifest model with permission enforcement and binary codec support for on-chain serialisation.
  - `NativeRegistry`/`NativeContract` traits enabling safe registration and invocation of native services.
  - `ExecutionContext`, `GasMeter`, and `Value` helper types that enforce storage access rules and gas accounting.
  - Built-in helpers for deleting state and sample native contracts that showcase reset semantics.
- **Dependencies**: `neo-base` for encoding primitives, `neo-crypto` for hashing, `neo-store` for key/value access, `dashmap` for concurrent registry updates.

## neo-vm
- **Role**: Lightweight stack-based virtual machine that evaluates Neo scripts and bridges to the native contract layer.
- **Key areas**:
  - Instruction enum and `VirtualMachine` interpreter with arithmetic, logic, storage slot and native-call support.
  - `VmValue` abstraction mirroring supported runtime types and conversion helpers for the adapter layer.
  - Test harness that wires a native contract registry through the VM to assert integration behaviour.
  - Stack manipulation and branching opcodes (`dup`, `swap`, `pick`, `roll`, `jump`, `jump_if_false`, comparisons) for structured control-flow.
  - Extended arithmetic and conversion helpers (`mod`, `negate`, `abs`, `sign`, `inc`, `dec`, `xor`, `shl`, `shr`, `to_bool`, `to_int`, `to_bytes`, `to_string`) matching the Neo N3 VM semantics.
  - Syscall dispatcher wiring prototype (`System.Runtime.Log/Notify/CheckWitness/Time/Platform/Trigger/GetInvocationCounter`, `System.Storage.{Get,Put,Delete}`) bridging into the contract execution context with storage-backed state.
- **Dependencies**: `neo-base` for byte wrappers, `neo-contract` for native invocation, optional `neo-store` for test scaffolding.

## neo-runtime
- **Role**: Orchestrate block execution, mempool management, and fee estimation for the node runtime.
- **Key areas**:
  - `Blockchain` tracker maintaining committed block summaries with rollback support.
  - `TxPool` FIFO queue providing deduplicated pending transactions and reservation helpers.
  - `FeeCalculator` combining base/per-byte fees with surge multipliers derived from load.
  - `Runtime` facade exposing a cohesive API for higher-level services.
- **Dependencies**: `neo-base` for hash primitives.

## neo-wallet
- **Role**: Manage account metadata, signing keys and encrypted keystore material for client-facing tooling.
- **Key areas**:
  - `Account` model supporting watch-only and signing keys with script-hash derivation.
  - Deterministic wallet container with duplicate checks and batch keystore export.
  - NEP-2 compatible keystore builder using scrypt key stretching and AES-256-ECB encryption, plus persistence helpers.
  - Integrity-checked keystore import that reconstructs accounts and rejects tampered entries.
  - Wallet storage adapter that loads/saves encrypted accounts via the `neo-store` backend.
- **Dependencies**: `neo-base` for hashing and byte utilities, `neo-crypto` for secp256r1/AES/scrypt primitives, `neo-store` for persistence in std builds, `rand` for deterministic fixture salts.

## neo-node
- **Role**: Provide a Tokio-driven orchestration layer that wires consensus, storage, and networking into a runnable node process.
- **Key areas**:
  - Configurable runtime with HTTP status endpoint (`/status`) exposing node telemetry.
  - Background services stub where P2P, consensus, and storage loops can be integrated.
  - Library API (`run(NodeConfig)`) plus binary entrypoint for launching the node from the command line.
  - Consensus snapshot loading/persisting backed by `neo-store`, ensuring restarts resume from the last committed height.
  - Consensus telemetry endpoint (`/consensus`) exposing participation, tallies, quorum, and missing validators for observability.
- Stage-aware expectation metrics (expected validator counts per message kind, e.g. commit expectations follow the validators that issued `PrepareResponse`) so clients can differentiate inactive phases from satisfied quorums.
- Stage status cache persisted in consensus snapshots and surfaced via telemetry (`inactive` / `pending` / `complete`) without replaying prior messages, including freshness (`age_ms`) so operators can flag stale phases or override thresholds on demand (`/consensus?stale_threshold_ms=`).
- Telemetry includes the currently scheduled primary validator and the full validator roster (id, optional alias, compressed key, script hash) so operators immediately know who should be broadcasting `PrepareRequest` messages.
- `NodeConfig` lets operators specify validator metadata (id, Secp256r1 key, optional alias) via config files; the node hydrates consensus state with those values instead of synthetic defaults.
  - Wallet API (`/wallet/accounts`, `/wallet/pending`) surfacing stored script hashes and mempool identifiers.
- **Dependencies**: `neo-base`, `neo-crypto`, `neo-store`, `neo-consensus`, `neo-p2p`, `axum`, `tokio`, `tracing`.

## neo-cli
- **Role**: Command line companion that interacts with `neo-node` via HTTP endpoints for operational visibility.
- **Key areas**:
  - Clap-powered interface with subcommands (`status`, `consensus`, `wallet accounts`, `wallet pending`) to query node telemetry, consensus participation, and wallet activity.
- Stage summary rendering that highlights whether each consensus phase is inactive, in-progress, or complete based on the node's expectation metrics.
- CLI output surfaces the live primary validator (marked with an asterisk) and validator roster (aliases + script hashes + keys) alongside expected/missing counts per stage to mirror the node telemetry, and can export the roster via `neo consensus --export-validators <path>`.
  - Async HTTP client layered on top of `reqwest` reusing `neo-node` response types for consistent decoding.
- **Dependencies**: `neo-node`, `tokio`, `reqwest`, `clap`, `serde`, `anyhow`.

## Verification checklist
- Each crate must compile with `default-features = []`.
- `neo-base` and `neo-crypto` operate in `no_std` mode (with alloc).
- Unit tests cover the documented API surfaces (handshake roundtrip, storage put/get, Merkle root calculation, etc.).
- Documentation comments link back to this design and mention any intentionally unimplemented portions.

## Integration sketch

```rust
use neo_base::{Bytes, Hash256};
use neo_p2p::{
    build_version_payload, handshake::HandshakeRole, message::Endpoint, Message, NeoMessageCodec,
    Peer, PeerEvent,
};
use neo_p2p::message::{InventoryItem, InventoryKind, InventoryPayload, PayloadWithData};
use neo_store::{BlockRecord, Blocks, Column, HeaderRecord, Headers, MemoryStore, StoreExt};

// Start with a clean in-memory store and seed a header record.
let store = MemoryStore::with_columns(&[Headers::ID, Blocks::ID]);
let header = HeaderRecord {
    hash: Hash256::new([0u8; 32]),
    height: 0,
    raw: Bytes::from(vec![0x01, 0x02, 0x03]),
};
store.put_encoded(Headers::ID, &header.key(), &header)?;

// Prepare a block payload ready to broadcast once peers request it.
let block = BlockRecord {
    hash: header.hash,
    raw: Bytes::from(vec![0x04, 0x05]),
};

// Drive the P2P handshake and publish inventory.
let local_version = build_version_payload(
    860_833_102,
    0x03,
    1,
    Endpoint::new("0.0.0.0".parse()?, 20333),
    Endpoint::new("0.0.0.0".parse()?, 20334),
    header.height,
);
let remote = Endpoint::new("127.0.0.1".parse()?, 20335);
let mut peer = Peer::outbound(remote, local_version);
for outbound in peer.bootstrap() {
    send_message(outbound);
}

if let PeerEvent::HandshakeCompleted = peer.on_message(Message::Verack)? {
    let inv = Message::Inventory(InventoryPayload::new(vec![InventoryItem {
        kind: InventoryKind::Block,
        hash: block.hash,
    }]));
    send_message(inv);
}

// Encode/decode messages over the wire with the shared codec.
let mut codec = NeoMessageCodec::new();
let payload = PayloadWithData::new(block.hash, block.raw.clone());
codec.encode(Message::Transaction(payload), &mut socket_buf)?;
```
