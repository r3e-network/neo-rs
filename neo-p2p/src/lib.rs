// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # Neo P2P
//!
//! P2P protocol types and networking primitives for the Neo blockchain.
//!
//! This crate provides the foundational types and abstractions for Neo's peer-to-peer
//! networking layer. It is designed as a lightweight, dependency-minimal crate for
//! external consumers who need P2P protocol types without the full networking stack.
//!
//! ## Architecture
//!
//! The crate is split into two usage modes:
//!
//! ### 1. Basic Types (this crate)
//!
//! For lightweight applications that only need protocol types:
//!
//! ```rust
//! use neo_p2p::{MessageCommand, InventoryType, VerifyResult};
//!
//! let cmd = MessageCommand::GetBlocks;
//! let inv = InventoryType::Transaction;
//! ```
//!
//! ### 2. Full Networking (neo-core)
//!
//! For full P2P node functionality, use `neo_core::network::p2p`:
//!
//! ```rust,no_run
//! use neo_core::network::p2p::LocalNode;
//!
//! // Full P2P node with connection management
//! let node = LocalNode::new(config);
//! ```
//!
//! ## Layer Position
//!
//! This crate is part of **Layer 1 (Core)** in the neo-rs architecture:
//!
//! ```text
//! Layer 2 (Service): neo-chain, neo-mempool
//!            │
//!            ▼
//! Layer 1 (Core):   neo-p2p ◄── YOU ARE HERE
//!            │
//!            ▼
//! Layer 0 (Foundation): neo-primitives
//! ```
//!
//! ## When to Use This Crate
//!
//! Use `neo-p2p` directly when you need:
//! - Message command enums
//! - Inventory types
//! - Verification results
//! - Witness conditions
//! - **Minimal dependencies** (only `neo-primitives`)
//!
//! Use `neo_core::network::p2p` when you need:
//! - Full P2P node implementation
//! - Connection management
//! - Block/transaction synchronization
//! - Peer discovery
//!
//! ## Core Types
//!
//! | Type | Purpose | Example |
//! |------|---------|---------|
//! | [`MessageCommand`] | P2P message identifiers | `MessageCommand::GetBlocks` |
//! | [`InventoryType`] | Inventory type codes | `InventoryType::Transaction` |
//! | [`VerifyResult`] | Verification outcomes | `VerifyResult::Succeed` |
//! | [`WitnessScope`] | Signature scopes | `WitnessScope::CalledByEntry` |
//! | [`WitnessConditionType`] | Condition types | `WitnessConditionType::And` |
//! | [`NodeCapabilityType`] | Node capabilities | `NodeCapabilityType::FullNode` |
//! | [`OracleResponseCode`] | Oracle response codes | `OracleResponseCode::Success` |
//! | [`TransactionAttributeType`] | TX attribute types | `TransactionAttributeType::Url` |
//! | [`TransactionRemovalReason`] | Mempool removal reasons | `TransactionRemovalReason::Capacity` |
//! | [`ContainsTransactionType`] | TX containment status | `ContainsTransactionType::Valid` |
//!
//! ## Example
//!
//! ```rust
//! use neo_p2p::{
//!     MessageCommand, InventoryType, VerifyResult, WitnessScope,
//! };
//!
//! // Parse message command from byte
//! let cmd = MessageCommand::from_byte(0x2b);
//! assert_eq!(cmd, MessageCommand::GetData);
//!
//! // Convert inventory type to message command
//! let inv = InventoryType::Block;
//! let cmd: MessageCommand = inv.into();
//! assert_eq!(cmd, MessageCommand::Block);
//!
//! // Check verification result
//! let result = VerifyResult::Succeed;
//! assert!(result.is_success());
//!
//! // Work with witness scopes
//! let scope = WitnessScope::CalledByEntry;
//! assert!(scope.is_valid());
//! ```
//!
//! ## Network Protocol
//!
//! Neo uses a custom P2P protocol over TCP. Messages follow this structure:
//!
//! ```text
//! ┌──────────┬──────────┬──────────┬──────────┐
//! │  Magic   │ Command  │  Length  │ Payload  │
//! │ (4 bytes)│ (12 bytes│ (4 bytes)│  (var)   │
//! │          │  ASCII)  │          │          │
//! └──────────┴──────────┴──────────┴──────────┘
//! ```
//!
//! ### Message Commands
//!
//! | Command | Value | Description |
//! |---------|-------|-------------|
//! | `Version` | 0x00 | Protocol version handshake |
//! | `Verack` | 0x01 | Version acknowledgment |
//! | `GetAddr` | 0x02 | Request peer addresses |
//! | `Addr` | 0x03 | Peer address list |
//! | `Ping` | 0x18 | Keepalive ping |
//! | `Pong` | 0x19 | Keepalive pong |
//! | `GetHeaders` | 0x20 | Request block headers |
//! | `Headers` | 0x21 | Block header list |
//! | `GetBlocks` | 0x22 | Request blocks |
//! | `Block` | 0x2a | Block data |
//! | `Tx` | 0x2b | Transaction |
//! | `Consensus` | 0x2c | dBFT consensus message |
//! | `Inv` | 0x27 | Inventory announcement |
//! | `GetData` | 0x28 | Request inventory data |
//! | `Reject` | 0x26 | Reject message |
//!
//! ## Feature Flags
//!
//! - `std` (default): Standard library support
//! - `serde`: Serialization support
//!
//! ## Error Handling
//!
//! All fallible operations return [`P2PResult`]:
//!
//! ```rust,no_run
//! use neo_p2p::{P2PError, P2PResult};
//!
//! fn parse_command(byte: u8) -> P2PResult<MessageCommand> {
//!     MessageCommand::try_from(byte)
//! }
//! ```

// ============================================================================
// Module Declarations
// ============================================================================

/// Transaction containment type enumeration.
pub mod contains_transaction_type;

/// Error types and result handling.
pub mod error;

/// Inventory type enumeration (Block, Transaction, etc.).
pub mod inventory_type;

/// P2P message command identifiers.
pub mod message_command;

/// Message header flags.
pub mod message_flags;

/// Node capability type enumeration.
pub mod node_capability_type;

/// Oracle response code enumeration.
pub mod oracle_response_code;

/// P2P trait definitions.
pub mod traits;

/// Transaction removal reason enumeration.
pub mod transaction_removal_reason;

/// Verification result enumeration.
pub mod verify_result;

/// Witness condition type enumeration.
pub mod witness_condition_type;

/// Witness rule action enumeration.
pub mod witness_rule_action;

// ============================================================================
// Public Re-exports
// ============================================================================

// Core types from this crate
pub use contains_transaction_type::ContainsTransactionType;
pub use error::{P2PError, P2PResult};
pub use inventory_type::InventoryType;
pub use message_command::MessageCommand;
pub use message_flags::MessageFlags;
pub use node_capability_type::NodeCapabilityType;
pub use oracle_response_code::OracleResponseCode;
pub use transaction_removal_reason::TransactionRemovalReason;
pub use verify_result::VerifyResult;
pub use witness_condition_type::WitnessConditionType;
pub use witness_rule_action::WitnessRuleAction;

// Re-exports from neo-primitives
pub use neo_primitives::{InvalidWitnessScopeError, TransactionAttributeType, WitnessScope};

// P2P traits for implementing network services
pub use traits::{
    Broadcaster, DataRequester, P2PConfig, P2PEvent, P2PEventSubscriber, P2PService, PeerInfo,
    PeerManager,
};
