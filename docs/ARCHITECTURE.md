# Neo-rs Architecture Documentation

> **Version**: 0.7.0  
> **Last Updated**: 2026-01-28  
> **Target Compatibility**: Neo N3 v3.9.2

This document provides comprehensive architecture documentation for the neo-rs project, a professional Rust implementation of the Neo N3 blockchain node.

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Core Components](#2-core-components)
3. [Data Flow](#3-data-flow)
4. [Module Structure](#4-module-structure)
5. [Security Architecture](#5-security-architecture)
6. [Appendix](#6-appendix)

---

## 1. System Overview

### 1.1 High-Level Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                                    APPLICATION LAYER                                        │
│                                                                                             │
│   ┌─────────────────────────────────┐    ┌─────────────────────────────────┐                │
│   │           neo-cli               │    │           neo-node              │                │
│   │        (CLI Client)             │    │       (Node Daemon)             │                │
│   │                                 │    │                                 │                │
│   │  • Wallet management            │    │  • P2P networking               │                │
│   │  • Contract invocation          │    │  • RPC server                   │                │
│   │  • Transaction building         │    │  • Consensus participation      │                │
│   │  • Query operations             │    │  • Block synchronization        │                │
│   │  • Offline signing              │    │  • Health/metrics endpoints     │                │
│   └─────────────────────────────────┘    └─────────────────────────────────┘                │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
                                                   │
                                                   ▼
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                                    SERVICE LAYER                                            │
│                                                                                             │
│   ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐              │
│   │     neo-chain        │  │    neo-mempool       │  │     neo-state        │              │
│   │                      │  │                      │  │                      │              │
│   │  • Chain management  │  │  • Transaction pool  │  │  • World state       │              │
│   │  • Fork choice       │  │  • Fee prioritization│  │  • Account state     │              │
│   │  • Block validation  │  │  • Conflict detection│  │  • Contract storage  │              │
│   │  • Reorganization    │  │  • Expiration        │  │  • State snapshots   │              │
│   └──────────────────────┘  └──────────────────────┘  └──────────────────────┘              │
│                                                                                             │
│   ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐              │
│   │    neo-config        │  │   neo-telemetry      │  │  neo-storage (impl)  │              │
│   │                      │  │                      │  │                      │              │
│   │  • Protocol settings │  │  • Metrics           │  │  • RocksDB backend   │              │
│   │  • Network config    │  │  • Health checks     │  │  • Cache layers      │              │
│   │  • Node configuration│  │  • Tracing           │  │  • Memory store      │              │
│   └──────────────────────┘  └──────────────────────┘  └──────────────────────┘              │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
                                                   │
                                                   ▼
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                                     CORE LAYER                                              │
│                                                                                             │
│   ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐              │
│   │      neo-core        │  │       neo-vm         │  │      neo-p2p         │              │
│   │                      │  │                      │  │                      │              │
│   │  • Protocol types    │  │  • Execution engine  │  │  • Message types     │              │
│   │  • Ledger (blocks/tx)│  │  • Instruction set   │  │  • P2P protocol      │              │
│   │  • Smart contracts   │  │  • Stack machine     │  │  • Handshake         │              │
│   │  • Native contracts  │  │  • Gas metering      │  │  • Inventory mgmt    │              │
│   │  • Wallets/keys      │  │  • Debugging         │  │  • Peer management   │              │
│   └──────────────────────┘  └──────────────────────┘  └──────────────────────┘              │
│                                                                                             │
│   ┌──────────────────────┐  ┌──────────────────────┐                                        │
│   │   neo-consensus      │  │      neo-rpc         │                                        │
│   │                      │  │                      │                                        │
│   │  • dBFT 2.0 algorithm│  │  • JSON-RPC server   │                                        │
│   │  • Consensus messages│  │  • RPC client        │                                        │
│   │  • View changes      │  │  • Method handlers   │                                        │
│   │  • Block signing     │  │  • Typed APIs        │                                        │
│   └──────────────────────┘  └──────────────────────┘                                        │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
                                                   │
                                                   ▼
┌─────────────────────────────────────────────────────────────────────────────────────────────┐
│                                   FOUNDATION LAYER                                          │
│                                                                                             │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│   │neo-primitives│  │  neo-crypto  │  │ neo-storage  │  │    neo-io    │  │   neo-json   │  │
│   │              │  │              │  │   (traits)   │  │              │  │              │  │
│   │ • UInt160    │  │ • SHA256     │  │ • IStore     │  │ • Binary RW  │  │ • JToken     │  │
│   │ • UInt256    │  │ • Hash160    │  │ • ISnapshot  │  │ • Serialize  │  │ • JObject    │  │
│   │ • BigDecimal │  │ • ECC (P-256)│  │ • DataCache  │  │ • ISerializ  │  │ • JArray     │  │
│   │ • Hardfork   │  │ • MPT Trie   │  │ • StorageKey │  │ • MemoryReader│  │ • JPath     │  │
│   └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Component Relationships

```
                    ┌─────────────────────────────────────┐
                    │           Client Request            │
                    └───────────────┬─────────────────────┘
                                    │
                    ┌───────────────▼─────────────────────┐
                    │         JSON-RPC Server             │
                    │         (neo-rpc/server)            │
                    └───────────────┬─────────────────────┘
                                    │
            ┌───────────────────────┼───────────────────────┐
            │                       │                       │
            ▼                       ▼                       ▼
┌───────────────────┐   ┌───────────────────┐   ┌───────────────────┐
│  Blockchain Query │   │ Transaction Submit│   │ Contract Invoke   │
│                   │   │                   │   │                   │
│ • getblock        │   │ • sendrawtx       │   │ • invokefunction  │
│ • getblockheader  │   │                   │   │ • invokescript    │
└─────────┬─────────┘   └─────────┬─────────┘   └─────────┬─────────┘
          │                       │                       │
          ▼                       ▼                       ▼
┌───────────────────┐   ┌───────────────────┐   ┌───────────────────┐
│   neo-chain       │   │   neo-mempool     │   │     neo-vm        │
│                   │   │                   │   │                   │
│ • Block storage   │   │ • Tx validation   │   │ • Script exec     │
│ • Chain state     │   │ • Fee ordering    │   │ • Gas metering    │
│ • Fork choice     │   │ • Conflict check  │   │ • Stack machine   │
└─────────┬─────────┘   └─────────┬─────────┘   └─────────┬─────────┘
          │                       │                       │
          └───────────────────────┼───────────────────────┘
                                  │
                                  ▼
                    ┌─────────────────────────────┐
                    │        neo-core             │
                    │    (Protocol & Ledger)      │
                    └─────────────┬───────────────┘
                                  │
                    ┌─────────────┴───────────────┐
                    │                             │
                    ▼                             ▼
        ┌───────────────────┐         ┌───────────────────┐
        │   neo-consensus   │         │     neo-p2p       │
        │    (dBFT 2.0)     │         │  (Networking)     │
        │                   │         │                   │
        │ • Block proposal  │         │ • Peer discovery  │
        │ • Vote collection │         │ • Block sync      │
        │ • Commit signing  │         │ • Tx relay        │
        │ • View changes    │         │ • Consensus msg   │
        └───────────────────┘         └───────────────────┘
```

---

## 2. Core Components

### 2.1 VM (Virtual Machine)

The Neo Virtual Machine (NeoVM) is a lightweight, stack-based virtual machine for executing smart contracts.

#### 2.1.1 Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    ApplicationEngine                             │
│         (High-level interface with blockchain integration)       │
│  • Trigger types (Application, Verification, System)            │
│  • Gas limit enforcement                                        │
│  • Interop service registration                                 │
│  • Notification events                                          │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ExecutionEngine                               │
│              (Core VM: stack, contexts, execution loop)          │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐    │
│  │ Evaluation  │  │   Context    │  │    Reference         │    │
│  │   Stack     │  │   Stack      │  │    Counter           │    │
│  │             │  │              │  │   (GC support)       │    │
│  │ • Push/Pop  │  │ • Call frames│  │ • Track references   │    │
│  │ • Type-safe │  │ • Locals     │  │ • Cycle detection    │    │
│  │ • Limits    │  │ • Static vars│  │ • Memory mgmt        │    │
│  └─────────────┘  └──────────────┘  └──────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    JumpTable                                     │
│            (Opcode implementations and dispatch)                 │
└─────────────────────────────────────────────────────────────────┘
```

#### 2.1.2 Instruction Set

The VM implements the complete Neo N3 instruction set organized by category:

| Category | Opcodes | Description |
|----------|---------|-------------|
| **Push** | `PUSH0`-`PUSH16`, `PUSHDATA1`-`PUSHDATA4` | Push constants onto stack |
| **Control** | `JMP`, `JMPIF`, `JMPIFNOT`, `CALL`, `CALL_L`, `RET`, `SYSCALL` | Flow control and method calls |
| **Stack** | `DUP`, `DROP`, `NIP`, `XDROP`, `PICK`, `ROLL`, `ROT`, `SWAP`, `TUCK`, `OVER`, `PICKITEM`, `SETITEM`, `REVERSE` | Stack manipulation |
| **Numeric** | `ADD`, `SUB`, `MUL`, `DIV`, `MOD`, `INC`, `DEC`, `SIGN`, `ABS`, `NEGATE` | Arithmetic operations |
| **Bitwise** | `INVERT`, `AND`, `OR`, `XOR`, `SHL`, `SHR`, `NOT`, `BOOLAND`, `BOOLOR`, `NZ` | Bitwise logic |
| **Splice** | `NEWBUFFER`, `MEMCPY`, `CAT`, `SUBSTR`, `LEFT`, `RIGHT` | Byte string operations |
| **Compound** | `NEWARRAY`, `NEWARRAY_T`, `NEWSTRUCT`, `NEWMAP`, `PACK`, `UNPACK`, `SIZE`, `HASKEY`, `KEYS`, `VALUES`, `PICKITEM`, `SETITEM`, `REMOVE`, `CLEARITEMS` | Complex types (arrays, maps) |
| **Slot** | `LDLOC`, `STLOC`, `LDSFLD`, `STSFLD`, `LDARG`, `STARG` | Local and static variable access |
| **Types** | `ISNULL`, `ISTYPE`, `CONVERT` | Type checking and conversion |

#### 2.1.3 Stack Items

```rust
pub enum StackItem {
    Boolean(bool),           // True/false values
    Integer(i32),            // 32-bit signed integers
    ByteString(Vec<u8>),     // Byte arrays
    Buffer(Vec<u8>),         // Mutable byte buffers
    Array(Vec<StackItem>),   // Arrays (reference type)
    Struct(Vec<StackItem>),  // Structs (value type)
    Map(HashMap<StackItem, StackItem>), // Key-value maps
    Pointer(usize),          // Code pointers
    InteropInterface(Box<dyn Any>), // Native object references
}
```

#### 2.1.4 Execution States

| State | Description |
|-------|-------------|
| `HALT` | Execution completed successfully |
| `FAULT` | Execution failed (exception, out of gas) |
| `BREAK` | Hit a breakpoint (debugging) |
| `NONE` | Not yet started |

### 2.2 Storage (RocksDB & Caching)

#### 2.2.1 Storage Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Application Layer                             │
│         (Smart contracts, native contracts, ledger)              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    DataCache                                     │
│              (In-memory caching with change tracking)            │
│  • TrackState: None, Added, Changed, Deleted, NotFound          │
│  • Write-through to underlying store                            │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    ISnapshot                                     │
│           (Point-in-time view with seek/find operations)         │
│  • Multi-level snapshot isolation                               │
│  • Forward/backward iteration                                   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    IStore                                        │
│              (Key-value storage abstraction)                     │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────────┐         ┌──────────────────┐              │
│  │   RocksDBStore   │         │   MemoryStore    │              │
│  │                  │         │                  │              │
│  │ • Persistent     │         │ • In-memory      │              │
│  │ • Column families│         │ • Testing        │              │
│  │ • Snapshots      │         │ • Ephemeral      │              │
│  └──────────────────┘         └──────────────────┘              │
└─────────────────────────────────────────────────────────────────┘
```

#### 2.2.2 Storage Key Structure

```
┌────────────────────────────────────────────────────────────────┐
│                     StorageKey Format                           │
├────────────────────────────────────────────────────────────────┤
│  [4 bytes]    [1 byte]    [variable]                           │
│  Contract ID  Prefix      Key Suffix                           │
│                                                                │
│  Example keys:                                                 │
│  • Account:     [0xFFFFFFFD][0x14][UInt160 bytes]              │
│  • Contract:    [0xFFFFFFFF][0x00][Contract hash]              │
│  • Storage:     [Contract ID][0x00][Storage key]               │
└────────────────────────────────────────────────────────────────┘
```

#### 2.2.3 Merkle Patricia Trie (MPT)

The MPT provides cryptographically verifiable state proofs:

```
┌─────────────────────────────────────────────────────────────────┐
│                    MPT Trie Structure                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│                         [Root Hash]                             │
│                              │                                  │
│              ┌───────────────┼───────────────┐                  │
│              │               │               │                  │
│         [Branch]        [Branch]        [Leaf]                  │
│         /  |  \            |                                  │
│       [L] [E] [A]      [Extension]                            │
│                          /       \                             │
│                       [Leaf]    [Branch]                        │
│                                                                 │
│  Node Types:                                                    │
│  • Branch: 16 children + optional value                        │
│  • Extension: Shared prefix + next node                        │
│  • Leaf: Key suffix + value                                    │
│  • Hash: Reference to node stored elsewhere                    │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 2.3 Network (P2P Protocol)

#### 2.3.1 P2P Message Structure

```
┌─────────────────────────────────────────────────────────────────┐
│                    P2P Message Format                            │
├──────────┬──────────┬──────────┬────────────────────────────────┤
│  Magic   │ Command  │  Length  │     Payload (compressed)       │
│ 4 bytes  │12 bytes  │ 4 bytes  │       (variable)               │
│  Network │  ASCII   │  uint32  │                                │
│   ID     │   name   │          │                                │
└──────────┴──────────┴──────────┴────────────────────────────────┘

Network Magic Numbers:
• MainNet: 0x00746E41 (NtA in LE)
• TestNet: 0x74746E41 (NtT in LE)
• Private: Configurable
```

#### 2.3.2 Message Types

| Command | Value | Direction | Description |
|---------|-------|-----------|-------------|
| `Version` | 0x00 | Bidirectional | Protocol version handshake |
| `Verack` | 0x01 | Bidirectional | Version acknowledgment |
| `GetAddr` | 0x02 | Outbound | Request peer addresses |
| `Addr` | 0x03 | Inbound | Peer address list |
| `Ping` | 0x18 | Bidirectional | Keepalive ping |
| `Pong` | 0x19 | Bidirectional | Keepalive pong |
| `GetHeaders` | 0x20 | Outbound | Request block headers |
| `Headers` | 0x21 | Inbound | Block header list |
| `GetBlocks` | 0x22 | Outbound | Request blocks |
| `GetData` | 0x28 | Outbound | Request inventory data |
| `Inv` | 0x27 | Inbound | Inventory announcement |
| `Block` | 0x2a | Inbound | Block data |
| `Tx` | 0x2b | Inbound | Transaction |
| `Consensus` | 0x2c | Bidirectional | dBFT consensus message |
| `Reject` | 0x26 | Inbound | Rejection message |
| `FilterLoad` | 0x12 | Outbound | Load Bloom filter |
| `FilterAdd` | 0x13 | Outbound | Add to Bloom filter |
| `FilterClear` | 0x14 | Outbound | Clear Bloom filter |

#### 2.3.3 Inventory Types

```rust
pub enum InventoryType {
    Transaction = 0x01,    // Transaction hash
    Block = 0x02,          // Block hash
    Consensus = 0xE0,      // Consensus payload hash
}
```

#### 2.3.4 Node Capability Types

```rust
pub enum NodeCapabilityType {
    FullNode = 0x01,       // Stores full blockchain
    LightNode = 0x02,      // Headers-only SPV mode
    Bootstrap = 0x03,      // Seeding node
}
```

### 2.4 Consensus (dBFT 2.0 Algorithm)

#### 2.4.1 dBFT Overview

Delegated Byzantine Fault Tolerance is Neo's consensus algorithm providing:
- **Single-block finality**: Transactions are final once committed
- **Byzantine fault tolerance**: Tolerates up to f = (n-1)/3 malicious nodes
- **Rotating speaker**: Prevents centralization
- **View changes**: Recovers from failed speakers

#### 2.4.2 Consensus Flow

```
┌──────────────────────────────────────────────────────────────────────────┐
│                         dBFT 2.0 Consensus Flow                           │
│                                                                          │
│   Time │  Speaker (Primary)           Validators (Backups)               │
│   ─────┼────────────────────────────────────────────────────────────     │
│        │         │                           │                           │
│   T+0  │         │─── PrepareRequest ───────>│  (broadcast block)        │
│        │         │  [block, txs, timestamp]  │                           │
│        │         │                           │                           │
│   T+?  │         │<── PrepareResponse ───────│  (ack with signature)     │
│        │         │        [signature]         │  Need M = (n+f)/2         │
│        │         │                           │                           │
│   T+?  │         │<──────── Commit ──────────│  (after M responses)      │
│        │         │         [signature]        │                           │
│        │         │                           │                           │
│   T+?  │         │      Block Committed      │  (after M commits)        │
│        │         ▼                           ▼                           │
│                                                                          │
│  M = Minimum consensus nodes = (n + f) / 2 + 1 = 2f + 1                  │
│                                                                          │
│  Where:                                                                  │
│    n = Total validators                                                  │
│    f = floor((n-1)/3) = max Byzantine nodes                              │
│    M = Minimum signatures needed for consensus                           │
└──────────────────────────────────────────────────────────────────────────┘
```

#### 2.4.3 Consensus Message Types

| Message | Purpose | Sender | Fields |
|---------|---------|--------|--------|
| `PrepareRequest` | Propose new block | Speaker | Block, transactions, timestamp, nonce |
| `PrepareResponse` | Acknowledge proposal | Validator | Signature, validator index |
| `Commit` | Agree to commit | Any validator | Signature, validator index |
| `ChangeView` | Request view change | Any validator | New view number, reason |
| `RecoveryRequest` | Request state sync | Any validator | Block index |
| `RecoveryMessage` | Provide state for sync | Any validator | Consensus state, signatures |

#### 2.4.4 Change View Reasons

```rust
pub enum ChangeViewReason {
    Timeout = 0x00,                // Speaker didn't respond in time
    TxNotFound = 0x01,             // Referenced transaction missing
    TxRejectedByPolicy = 0x02,     // Transaction failed policy check
    TxInvalid = 0x03,              // Transaction verification failed
    BlockRejectedByPolicy = 0x04,  // Block failed policy check
    BlockInvalid = 0x05,           // Block verification failed
    ChangeAgreement = 0x06,        // Agreed with other validators
}
```

#### 2.4.5 Speaker Selection

```rust
// Speaker rotates deterministically based on view number
speaker_index = (block_height + view_number) % num_validators;
speaker = validators[speaker_index];
```

#### 2.4.6 Timing Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `BlockTime` | 15s | Target block interval |
| `PrepareRequestTimeout` | 4s | Wait for `PrepareRequest` |
| `PrepareResponseTimeout` | 4s | Wait for `PrepareResponses` |
| `CommitTimeout` | 4s | Wait for `Commits` |
| `ViewChangeTimeout` | 4s | Wait for view changes |

---

## 3. Data Flow

### 3.1 Transaction Lifecycle

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        TRANSACTION LIFECYCLE                                 │
└─────────────────────────────────────────────────────────────────────────────┘

Phase 1: Creation
─────────────────
     │
     ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   User/Wallet   │────▶│  Transaction    │────▶│   Sign with     │
│   Creates TX    │     │   Builder       │     │   Private Key   │
└─────────────────┘     └─────────────────┘     └────────┬────────┘
                                                         │
Phase 2: Submission                                       │
─────────────────                                         ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   neo-cli or    │◀────│  Signed TX      │◀────│   Witness       │
│   Wallet SDK    │     │  (serialized)   │     │  (Script +      │
│                 │     │                 │     │  Invocation)    │
└────────┬────────┘     └─────────────────┘     └─────────────────┘
         │
         │ JSON-RPC
         ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   RPC Server    │────▶│  Tx Validation  │────▶│   neo-mempool   │
│  (neo-rpc)      │     │  (Basic checks) │     │   (if valid)    │
└─────────────────┘     └─────────────────┘     └────────┬────────┘
                                                         │
Phase 3: Mempool Processing                               │
───────────────────────────                               ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Fee-ordered   │◀────│  Verification   │◀────│  Policy Checks  │
│   Tx Pool       │     │  (VM + Scripts) │     │  (Size, fees)   │
└────────┬────────┘     └─────────────────┘     └─────────────────┘
         │
         │ P2P Gossip
         ▼
┌─────────────────┐
│  Other Nodes'   │
│   Mempools      │
└─────────────────┘

Phase 4: Block Inclusion
────────────────────────
         │
         ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Consensus/     │────▶│   Block         │────▶│  Block          │
│  Miner Selects  │     │   Assembly      │     │  Propagation    │
│  Transactions   │     │   (Top fees)    │     │  (via P2P)      │
└─────────────────┘     └────────┬────────┘     └────────┬────────┘
                                 │                       │
Phase 5: Block Processing         │                       │
─────────────────────────         ▼                       ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Block Store    │◀────│  State Update   │◀────│  VM Execution   │
│  (Persistent)   │     │  (Accounts,     │     │  (Contracts)    │
│                 │     │   Storage)      │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                                 │
                                 ▼
                        ┌─────────────────┐
                        │  Event/Plugin   │
                        │  Notifications  │
                        │  (OnPersist)    │
                        └─────────────────┘

Phase 6: Finality
─────────────────
                                 │
                                 ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  dBFT Consensus │────▶│  Block Final    │────▶│  User Query     │
│  (M signatures) │     │  (Immutable)    │     │  (Confirmed)    │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

### 3.2 Block Processing Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     BLOCK PROCESSING PIPELINE                                │
└─────────────────────────────────────────────────────────────────────────────┘

  Received Block
       │
       ▼
┌─────────────────────────┐
│  1. Header Validation   │  • Hash integrity
│                         │  • Previous block hash links
│                         │  • Timestamp bounds
│                         │  • Merkle root verification
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│  2. Consensus Verify    │  • Validator signature count
│                         │  • Primary speaker check
│                         │  • View number verification
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│  3. TX Validation       │  • Verify each transaction
│                         │  • Check for duplicates
│                         │  • Validate script witnesses
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│  4. State Execution     │  • Execute transactions in order
│                         │  • Update account balances
│                         │  • Run smart contracts
│                         │  • Update contract storage
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│  5. State Root Update   │  • Compute new state root
│                         │  • Update Merkle Patricia Trie
│                         │  • Store state proof
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│  6. Persistence         │  • Write block to storage
│                         │  • Update chain index
│                         │  • Commit state changes
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│  7. Event Notification  │  • OnPersist event
│                         │  • Plugin notifications
│                         │  • P2P relay to peers
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│  8. Mempool Update      │  • Remove included TXs
│                         │  • Update pending TXs
│                         │  • Reverify remaining
└─────────────────────────┘
```

### 3.3 State Management

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          STATE MANAGEMENT                                    │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                            WorldState                                        │
│                                                                              │
│   ┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐ │
│   │    AccountState     │  │   ContractStorage   │  │   ContractState     │ │
│   │                     │  │                     │  │                     │ │
│   │ • NEO balance       │  │ • Key-value pairs   │  │ • Script hash       │ │
│   │ • GAS balance       │  │ • Storage items     │  │ • Bytecode          │ │
│   │ • Votes             │  │ • Merkle proofs     │  │ • Manifest          │ │
│   │ • Validator status  │  │ • Contract-bound    │  │ • Update counter    │ │
│   └─────────────────────┘  └─────────────────────┘  └─────────────────────┘ │
│                                                                              │
│   ┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐ │
│   │   Native Contracts  │  │   StateRoot         │  │   Validators        │ │
│   │                     │  │                     │  │                     │ │
│   │ • NeoToken          │  │ • Merkle root       │  │ • Registered        │ │
│   │ • GasToken          │  │ • Block height      │  │ • Elected           │ │
│   │ • Policy            │  │ • Timestamp         │  │ • Votes             │ │
│   │ • Ledger            │  │ • Proof available   │  │ • Commitee          │ │
│   └─────────────────────┘  └─────────────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ Snapshot Isolation
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         SnapshotManager                                      │
│                                                                              │
│   Transaction Execution:                                                     │
│   ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                     │
│   │   Read      │───▶│   Clone     │───▶│   Modify    │                     │
│   │   Snapshot  │    │   Snapshot  │    │   Snapshot  │                     │
│   └─────────────┘    └─────────────┘    └──────┬──────┘                     │
│                                                │                            │
│                                                ▼                            │
│                                        ┌─────────────┐                      │
│                                        │   Commit    │                      │
│                                        │   or Roll   │                      │
│                                        │   back      │                      │
│                                        └─────────────┘                      │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Module Structure

### 4.1 Crate Organization

```
neo-rs/
├── Cargo.toml                    # Workspace manifest
├── Cargo.lock                    # Dependency lockfile
│
├── neo-primitives/               # Foundation Layer
│   └── src/
│       ├── lib.rs                # Core re-exports
│       ├── uint160.rs            # 160-bit hash type
│       ├── uint256.rs            # 256-bit hash type
│       ├── hardfork.rs           # Protocol upgrades
│       └── ...
│
├── neo-crypto/                   # Foundation Layer
│   └── src/
│       ├── lib.rs
│       ├── hash.rs               # SHA256, RIPEMD160, etc.
│       ├── ecc.rs                # Elliptic curve types
│       ├── crypto_utils.rs       # Signing/verification
│       └── mpt_trie.rs           # Merkle Patricia Trie
│
├── neo-storage/                  # Foundation Layer
│   └── src/
│       ├── lib.rs
│       ├── traits.rs             # IStore, ISnapshot, IReadOnlyStore
│       ├── types.rs              # StorageKey, StorageItem
│       └── cache.rs              # DataCache implementation
│
├── neo-io/                       # Foundation Layer
│   └── src/
│       ├── lib.rs
│       ├── i_serializable.rs     # Serialization trait
│       ├── binary_writer.rs      # Binary encoding
│       └── caching/              # I/O caching utilities
│
├── neo-json/                     # Foundation Layer
│   └── src/
│       └── lib.rs                # JSON types (JToken, JObject)
│
├── neo-core/                     # Core Layer
│   └── src/
│       ├── lib.rs                # Main exports, IVerifiable trait
│       ├── ledger/               # Block, Transaction, Blockchain
│       │   ├── block.rs
│       │   ├── transaction.rs
│       │   ├── blockchain.rs
│       │   └── memory_pool.rs
│       ├── smart_contract/       # Contracts, Native contracts
│       │   ├── contract.rs
│       │   ├── manifest.rs
│       │   └── native/           # NeoToken, GasToken, etc.
│       ├── network/              # P2P payloads, messages
│       │   └── p2p/
│       ├── wallets/              # Wallet, KeyPair
│       ├── persistence/          # Storage abstractions
│       └── actors/               # Actor runtime (optional)
│
├── neo-vm/                       # Core Layer
│   └── src/
│       ├── lib.rs
│       ├── execution_engine.rs   # Core VM loop
│       ├── application_engine.rs # Blockchain-aware VM
│       ├── evaluation_stack.rs   # Operand stack
│       ├── execution_context.rs  # Call frames
│       ├── op_code/              # Opcode definitions
│       │   ├── mod.rs
│       │   └── op_code.rs        # All opcodes
│       ├── jump_table/           # Opcode implementations
│       │   ├── mod.rs
│       │   ├── push.rs
│       │   ├── control.rs
│       │   ├── numeric.rs
│       │   └── ...
│       ├── stack_item.rs         # VM value types
│       ├── script.rs             # Script container
│       └── interop_service.rs    # Native methods
│
├── neo-p2p/                      # Core Layer (types)
│   └── src/
│       ├── lib.rs
│       ├── message_command.rs    # P2P command enum
│       ├── inventory_type.rs     # Inventory types
│       └── verify_result.rs      # Verification results
│
├── neo-consensus/                # Core Layer
│   └── src/
│       ├── lib.rs
│       ├── service.rs            # ConsensusService (dBFT)
│       ├── context.rs            # ConsensusContext
│       ├── messages/             # Consensus message types
│       │   ├── prepare_request.rs
│       │   ├── prepare_response.rs
│       │   ├── commit.rs
│       │   └── change_view.rs
│       └── change_view_reason.rs
│
├── neo-rpc/                      # Core Layer
│   └── src/
│       ├── lib.rs
│       ├── server/               # RPC server (feature)
│       │   ├── mod.rs
│       │   └── methods/          # RPC method handlers
│       └── client/               # RPC client (feature)
│           ├── mod.rs
│           └── apis/             # Typed API wrappers
│
├── neo-chain/                    # Service Layer
│   └── src/
│       ├── lib.rs
│       ├── chain_state.rs        # Chain state machine
│       ├── fork_choice.rs        # Fork resolution
│       └── validation.rs         # Block validation
│
├── neo-mempool/                  # Service Layer
│   └── src/
│       ├── lib.rs
│       ├── pool.rs               # Mempool implementation
│       └── policy.rs             # Fee policies
│
├── neo-state/                    # Service Layer
│   └── src/
│       ├── lib.rs
│       ├── world_state.rs        # World state abstraction
│       ├── account.rs            # Account state
│       └── snapshot.rs           # Snapshot management
│
├── neo-config/                   # Service Layer
│   └── src/
│       └── lib.rs                # Configuration types
│
├── neo-telemetry/                # Service Layer
│   └── src/
│       └── lib.rs                # Metrics, tracing
│
├── neo-node/                     # Application Layer
│   └── src/
│       ├── main.rs               # Node daemon entry
│       ├── cli.rs                # CLI argument parsing
│       ├── startup.rs            # Node initialization
│       ├── consensus.rs          # Consensus integration
│       ├── rpc_consensus.rs      # RPC server setup
│       ├── health.rs             # Health check endpoints
│       └── metrics.rs            # Prometheus metrics
│
└── neo-cli/                      # Application Layer
    └── src/
        └── main.rs               # CLI client entry
```

### 4.2 Dependency Graph

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          DEPENDENCY GRAPH                                    │
│                                                                              │
│  Legend: ───▶ depends on                                                     │
└─────────────────────────────────────────────────────────────────────────────┘

Layer 3 (Application)
┌─────────────────────────────────────────────────────────────────┐
│  ┌──────────┐      ┌──────────┐                                  │
│  │neo-cli   │      │neo-node  │                                  │
│  └────┬─────┘      └────┬─────┘                                  │
└───────┼────────────────┼────────────────────────────────────────┘
        │                │
        │    ┌───────────┘
        │    │
        ▼    ▼
Layer 2 (Service)
┌─────────────────────────────────────────────────────────────────┐
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │neo-chain │  │neo-mempool│  │neo-state │  │neo-config│        │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘        │
│  ┌──────────┐  ┌──────────┐                                     │
│  │neo-telemetry│ │neo-tee │                                     │
│  └──────────┘  └──────────┘                                     │
└───────┼────────────────┼────────────────────────────────────────┘
        │                │
        └───────┬────────┘
                │
                ▼
Layer 1 (Core)
┌─────────────────────────────────────────────────────────────────┐
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │neo-core  │  │neo-vm    │  │neo-p2p   │  │neo-consensus│      │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘        │
│  ┌──────────┐  ┌──────────┐                                     │
│  │neo-rpc   │  │neo-hsm   │                                     │
│  └──────────┘  └──────────┘                                     │
└───────┼────────────────┼────────────────────────────────────────┘
        │                │
        └───────┬────────┘
                │
                ▼
Layer 0 (Foundation)
┌─────────────────────────────────────────────────────────────────┐
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │neo-primitives│ │neo-crypto│  │neo-storage│  │neo-io   │        │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘        │
│  ┌──────────┐                                                   │
│  │neo-json  │                                                   │
│  └──────────┘                                                   │
└─────────────────────────────────────────────────────────────────┘

Key Dependencies:
• neo-cli ──▶ neo-core, neo-rpc(client)
• neo-node ──▶ neo-core, neo-chain, neo-mempool, neo-consensus, neo-rpc(server)
• neo-core ──▶ neo-primitives, neo-crypto, neo-storage, neo-io, neo-vm, neo-json
• neo-vm ──▶ neo-primitives, neo-io
• neo-consensus ──▶ neo-primitives, neo-crypto
• neo-chain ──▶ neo-core, neo-state
• neo-state ──▶ neo-primitives, neo-storage
```

### 4.3 Feature Flags

| Crate | Feature | Description |
|-------|---------|-------------|
| `neo-core` | `runtime` | Actor-based runtime (`NeoSystem`, actors) |
| `neo-core` | `monitoring` | Metrics collection |
| `neo-rpc` | `server` | RPC server functionality |
| `neo-rpc` | `client` | RPC client functionality |
| `neo-vm` | `debug` | Debugging support |
| `neo-node` | `tee` | Trusted Execution Environment support |
| `neo-node` | `hsm` | Hardware Security Module support |

---

## 5. Security Architecture

### 5.1 Cryptographic Primitives

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       CRYPTOGRAPHIC PRIMITIVES                               │
└─────────────────────────────────────────────────────────────────────────────┘

Hash Functions
┌─────────────────────────────────────────────────────────────────────────────┐
│  Algorithm    │ Output Size │ Usage                                         │
├───────────────┼─────────────┼───────────────────────────────────────────────┤
│  SHA-256      │ 256 bits    │ Block/TX hashing, Merkle trees                │
│  SHA-512      │ 512 bits    │ Key derivation (BIP-32/39)                    │
│  RIPEMD-160   │ 160 bits    │ Script hash (Hash160)                         │
│  Hash160      │ 160 bits    │ RIPEMD160(SHA256(x)) - Addresses              │
│  Hash256      │ 256 bits    │ SHA256(SHA256(x)) - Checksums                 │
│  Keccak-256   │ 256 bits    │ Ethereum compatibility                        │
│  Blake2b      │ variable    │ Alternative hashing                           │
│  xxHash3      │ 32 bits     │ Storage key hashing (C# parity)               │
└─────────────────────────────────────────────────────────────────────────────┘

Elliptic Curve Cryptography
┌─────────────────────────────────────────────────────────────────────────────┐
│  Curve        │ Algorithm   │ Usage                                          │
├───────────────┼─────────────┼────────────────────────────────────────────────┤
│  secp256r1    │ ECDSA       │ Primary Neo N3 signatures                      │
│  secp256k1    │ ECDSA       │ Bitcoin/Ethereum compatibility                 │
│  Ed25519      │ EdDSA       │ Alternative signatures                         │
│  BLS12-381    │ BLS         │ Aggregate signatures, random oracle            │
└─────────────────────────────────────────────────────────────────────────────┘

Key Derivation
┌─────────────────────────────────────────────────────────────────────────────┐
│  BIP-32 (HD Wallets)                                                        │
│  └── Path: m/44'/888'/account'/change/index                                 │
│      • 44': BIP44 purpose                                                   │
│      • 888': Neo coin type                                                  │
│                                                                             │
│  BIP-39 (Mnemonics)                                                         │
│  └── 12-24 word phrases → seed → master key                                 │
│      • 10 language wordlists supported                                      │
│      • Optional passphrase protection                                       │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 Verification Pipelines

#### 5.2.1 Transaction Verification

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     TRANSACTION VERIFICATION PIPELINE                        │
└─────────────────────────────────────────────────────────────────────────────┘

Level 1: Syntax Validation (Lightweight)
┌─────────────────────────────────────────────────────────────────────────────┐
│  ✓ Size limits (≤ 102400 bytes)                                             │
│  ✓ Attribute count (≤ 16)                                                   │
│  ✓ Signer count (≤ 16)                                                      │
│  ✓ Witness count matches signers                                            │
│  ✓ Valid UTF-8 in strings                                                   │
│  ✓ Script parsable as VM bytecode                                           │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼ (if Level 1 passes)
Level 2: Policy Validation
┌─────────────────────────────────────────────────────────────────────────────┐
│  ✓ Sufficient network fee (≥ base fee + tx size * fee per byte)             │
│  ✓ Sufficient system fee (≥ execution cost estimate)                        │
│  ✓ Sender can cover fees                                                    │
│  ✓ Not already in blockchain                                                │
│  ✓ Not conflicting with mempool TXs                                         │
│  ✓ Not violating policy rules (blocked accounts, etc.)                      │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼ (if Level 2 passes)
Level 3: Script Verification (VM Execution)
┌─────────────────────────────────────────────────────────────────────────────┐
│  ✓ Witness scripts execute without fault                                    │
│  ✓ Verification scripts return TRUE                                         │
│  ✓ Gas limit not exceeded                                                   │
│  ✓ No invalid opcodes in verification context                               │
│  ✓ Witness scope restrictions honored                                       │
│     • None: No storage access                                               │
│     • CalledByEntry: Only entry call context                                │
│     • CustomContracts: Whitelist specific contracts                         │
│     • CustomGroups: Whitelist contract groups                               │
│     • Global: Full access (discouraged)                                     │
└─────────────────────────────────────────────────────────────────────────────┘
```

#### 5.2.2 Block Verification

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       BLOCK VERIFICATION PIPELINE                            │
└─────────────────────────────────────────────────────────────────────────────┘

Header Verification
┌─────────────────────────────────────────────────────────────────────────────┐
│  ✓ Hash meets difficulty requirement (if PoW, not in dBFT)                  │
│  ✓ Previous hash matches known tip                                          │
│  ✓ Timestamp within acceptable bounds (±15 min of network time)             │
│  ✓ Timestamp greater than previous block                                    │
│  ✓ Index = previous index + 1                                               │
│  ✓ Merkle root matches computed from transactions                           │
│  ✓ Size within limits                                                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
Consensus Verification (dBFT)
┌─────────────────────────────────────────────────────────────────────────────┐
│  ✓ Primary speaker matches expected for view                                │
│  ✓ Sufficient signatures from validators (M of N)                           │
│  ✓ Signatures valid against validator set                                   │
│  ✓ No duplicate validators in signatures                                    │
│  ✓ Block nonce consistent with view number                                  │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
Transaction Verification
┌─────────────────────────────────────────────────────────────────────────────┐
│  ✓ All transactions valid (see TX verification pipeline)                    │
│  ✓ No duplicate transactions in block                                       │
│  ✓ Primary TX (if any) valid                                                │
│  ✓ System fees cover block processing                                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
State Verification
┌─────────────────────────────────────────────────────────────────────────────┐
│  ✓ State root matches expected (if state root validation enabled)           │
│  ✓ Account balances consistent                                              │
│  ✓ Contract storage updates valid                                           │
│  ✓ Native contract invariants maintained                                    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.3 Security Mechanisms

#### 5.3.1 Witness Scopes

Witness scopes restrict what a transaction signature can authorize:

```rust
pub enum WitnessScope {
    None = 0x00,           // Only for transaction signing, no contracts
    CalledByEntry = 0x01,  // Only contracts called by entry script
    CustomContracts = 0x10,// Specific whitelisted contracts
    CustomGroups = 0x20,   // Specific whitelisted contract groups
    Global = 0x80,         // All contracts (discouraged, dangerous)
}
```

#### 5.3.2 Witness Rules

Fine-grained conditional authorization:

```rust
pub enum WitnessCondition {
    Boolean(bool),                    // True/False
    Not(Box<WitnessCondition>),       // Logical NOT
    And(Vec<WitnessCondition>),       // Logical AND
    Or(Vec<WitnessCondition>),        // Logical OR
    ScriptHash(UInt160),              // Specific contract hash
    Group(ECPoint),                   // Contract group public key
    CalledByEntry,                    // Called by entry point
    CalledByContract(UInt160),        // Called by specific contract
    CalledByGroup(ECPoint),           // Called by group member
}
```

#### 5.3.3 Sybil Resistance

| Mechanism | Implementation |
|-----------|----------------|
| Consensus | dBFT requires 2f+1 validators |
| Staking | NEO holders vote for validators |
| Cost | Transaction fees prevent spam |
| P2P | Connection limits, reputation |

### 5.4 Attack Mitigations

| Attack Vector | Mitigation |
|---------------|------------|
| **Double spend** | Single-block finality in dBFT |
| **51% attack** | Requires 2/3+ validators malicious |
| **Sybil attack** | Validator election requires NEO stake |
| **DDoS (P2P)** | Rate limiting, connection caps, Bloom filters |
| **DDoS (RPC)** | Request limits, authentication, CORS |
| **Replay attacks** | Unique transaction hashes, network magic |
| **Mempool flooding** | Fee prioritization, size limits, expiration |
| **VM exploits** | Gas limits, opcode limits, stack limits |
| **Storage bloat** | Storage rent, size limits on contracts |
| **Front-running** | Deterministic ordering by hash |

---

## 6. Appendix

### 6.1 C# Compatibility Matrix

| C# Namespace | Rust Crate | Rust Module |
|--------------|------------|-------------|
| `Neo` | `neo-core` | `neo_core` |
| `Neo.Cryptography` | `neo-crypto` | `neo_crypto` |
| `Neo.IO` | `neo-io` | `neo_io` |
| `Neo.Json` | `neo-json` | `neo_json` |
| `Neo.Ledger` | `neo-core` | `neo_core::ledger` |
| `Neo.Network.P2P` | `neo-p2p` | `neo_p2p` |
| `Neo.SmartContract` | `neo-core` | `neo_core::smart_contract` |
| `Neo.VM` | `neo-vm` | `neo_vm` |
| `Neo.Wallets` | `neo-core` | `neo_core::wallets` |
| `Neo.Plugins.RpcServer` | `neo-rpc` | `neo_rpc::server` |
| `Neo.Plugins.DBFTPlugin` | `neo-consensus` | `neo_consensus` |

### 6.2 Native Contract IDs

| Contract | ID | Hash (LE) |
|----------|-----|-----------|
| ContractManagement | -1 | `0xfffdc93764dbaddd97c48f252a53ea4643faa3fd` |
| StdLib | -2 | `0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0` |
| CryptoLib | -3 | `0x726cb6e0cd8628a1350a611384688911ab75f51b` |
| LedgerContract | -4 | `0xda65b600f7124ce6c79950c1772a36403104f2be` |
| NeoToken | -5 | `0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5` |
| GasToken | -6 | `0xd2a4cff31913016155e38e474a2c06d08be276cf` |
| PolicyContract | -7 | `0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b` |
| RoleManagement | -8 | `0x49cf4e5378ffcd4dec034fd98a174c5491e395e2` |
| OracleContract | -9 | `0xfe924b7cfe89ddd271abaf7210a80a7e11178758` |
| Notary | -10 | `0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b` |
| Treasury | -11 | `0x156326f25b1b5d839a4d326aeaa75383c9563ac1` |
| TokenManagement | -12 | `0xae00c57daeb20f9b65504f53265e4f32b9f4a8a0` |

### 6.3 Configuration Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `MaxTraceableBlocks` | 2102400 | ~1 year of blocks (15s interval) |
| `MaxTransactionsPerBlock` | 512 | Transaction limit per block |
| `MemoryPoolMaxTransactions` | 50000 | Maximum mempool size |
| `BlockTime` | 15000ms | Target block interval |
| `MaxBlockSize` | 4194304 | 4 MB maximum block size |
| `MaxTransactionSize` | 102400 | 100 KB maximum transaction |

### 6.4 Error Handling Hierarchy

```
┌─────────────────────────────────────────────────────────────────┐
│                    Error Type Hierarchy                          │
├─────────────────────────────────────────────────────────────────┤
│  Application Layer                                               │
│  ├── CliError (neo-cli)                                         │
│  └── NodeError (neo-node)                                       │
│                                                                  │
│  Service Layer                                                   │
│  ├── ChainError (neo-chain)                                     │
│  ├── MempoolError (neo-mempool)                                 │
│  └── StateError (neo-state)                                     │
│                                                                  │
│  Core Layer                                                      │
│  ├── CoreError (neo-core)                                       │
│  ├── VmError (neo-vm)                                           │
│  ├── P2PError (neo-p2p)                                         │
│  ├── RpcError (neo-rpc)                                         │
│  └── ConsensusError (neo-consensus)                             │
│                                                                  │
│  Foundation Layer                                                │
│  ├── PrimitiveError (neo-primitives)                            │
│  ├── CryptoError (neo-crypto)                                   │
│  ├── StorageError (neo-storage)                                 │
│  ├── IoError (neo-io)                                           │
│  └── JsonError (neo-json)                                       │
└─────────────────────────────────────────────────────────────────┘
```

### 6.5 Glossary

| Term | Definition |
|------|------------|
| **dBFT** | Delegated Byzantine Fault Tolerance - Neo's consensus algorithm |
| **MPT** | Merkle Patricia Trie - Cryptographic data structure for state |
| **VM** | Virtual Machine - Execution environment for smart contracts |
| **P2P** | Peer-to-Peer - Network communication between nodes |
| **RPC** | Remote Procedure Call - API for external interaction |
| **Witness** | Cryptographic proof authorizing a transaction |
| **Script Hash** | Hash of contract code (UInt160), identifies contracts |
| **Native Contract** | Built-in contracts (NeoToken, GasToken, etc.) |
| **Interop Service** | Native functions callable from VM |
| **Snapshot** | Point-in-time view of blockchain state |
| **Mempool** | Memory pool - Pending transactions |

---

## References

- [Neo N3 Documentation](https://docs.neo.org/)
- [Neo Whitepaper](https://docs.neo.org/docs/en-us/basic/whitepaper.html)
- [dBFT Whitepaper](https://docs.neo.org/docs/en-us/basic/consensus/whitepaper.html)
- [NeoVM Documentation](https://docs.neo.org/docs/en-us/reference/scapi/fw/dotnet.html)
- [Neo C# Reference Implementation](https://github.com/neo-project/neo)

---

*This documentation is maintained as part of the neo-rs project. For the most current information, refer to the source code and inline documentation.*
