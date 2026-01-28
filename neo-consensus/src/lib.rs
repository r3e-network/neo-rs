// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # Neo Consensus - dBFT 2.0 Implementation
//!
//! Delegated Byzantine Fault Tolerance consensus implementation for Neo N3.
//!
//! This crate provides a complete implementation of the dBFT 2.0 consensus algorithm,
//! which powers the Neo blockchain's block production. dBFT achieves single-block
//! finality while tolerating up to `f = (n-1)/3` Byzantine (malicious) nodes.
//!
//! ## Algorithm Overview
//!
//! dBFT (Delegated Byzantine Fault Tolerance) is a consensus mechanism designed
//! specifically for blockchains. It combines:
//!
//! - **Single-block finality**: Transactions are final once committed
//! - **Byzantine fault tolerance**: Works even with malicious nodes
//! - **Rotating speaker**: Prevents centralization
//! - **View changes**: Recovers from failed speakers
//!
//! ## Architecture
//!
//! ```
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     ConsensusService                             │
//! │              (Main state machine for dBFT 2.0)                   │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    ConsensusContext                              │
//! │         (Tracks view, validators, signatures, block)             │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  • View number      • Validator list    • Signatures             │
//! │  • Block data       • Timestamp         • ExpectedView           │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                  Consensus Messages                              │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  PrepareRequest  PrepareResponse  Commit  ChangeView  Recovery   │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Layer Position
//!
//! This crate is part of **Layer 1 (Core)** in the neo-rs architecture:
//!
//! ```
//! Layer 2 (Service): neo-chain
//!            │
//!            ▼
//! Layer 1 (Core):   neo-consensus ◄── YOU ARE HERE
//!            │
//!            ▼
//! Layer 0 (Foundation): neo-primitives, neo-crypto
//! ```
//!
//! ## Consensus Flow
//!
//! The dBFT consensus proceeds in views. Each view has a designated speaker
//! (primary) and multiple validators (backup nodes).
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                     dBFT 2.0 Consensus Flow                       │
//! │                                                                   │
//! │   Time │  Speaker                    Validators                    │
//! │   ─────┼──────────────────────────────────────────────────────    │
//! │        │    │                           │                         │
//! │   T+0  │    │─── PrepareRequest ───────>│  (propose block)        │
//! │        │    │  [block, txs, timestamp]  │                         │
//! │        │    │                           │                         │
//! │   T+?  │    │<── PrepareResponse ───────│  (validate & ack)       │
//! │        │    │        [signature]         │  M = (n+f)/2 responses  │
//! │        │    │                           │                         │
//! │   T+?  │    │<──────── Commit ──────────│  (when M responses)     │
//! │        │    │         [signature]        │                         │
//! │        │    │                           │                         │
//! │   T+?  │    │         Block Committed   │  (when M commits)       │
//! │        │    ▼                           ▼                         │
//! │        │                                                           │
//! │        │  [If timeout or invalid block: ChangeView]                │
//! │        │                                                           │
//! └──────────────────────────────────────────────────────────────────┘
//!
//! M = Minimum consensus nodes required = (n + f) / 2 + 1
//!   = (2n + 1) / 3  (same as 2f + 1)
//!
//! Where:
//!   n = Total number of validators
//!   f = Maximum Byzantine nodes = floor((n-1)/3)
//!   M = Minimum signatures needed
//! ```
//!
//! ## Message Types
//!
//! | Message | Purpose | Sender |
//! |---------|---------|--------|
//! | [`PrepareRequest`](PrepareRequestMessage) | Propose a new block | Speaker |
//! | [`PrepareResponse`](PrepareResponseMessage) | Acknowledge proposal | Validator |
//! | [`Commit`](CommitMessage) | Agree to commit block | Any validator |
//! | [`ChangeView`](ChangeViewMessage) | Request view change | Any validator |
//! | [`RecoveryRequest`](RecoveryRequestMessage) | Request state sync | Any validator |
//! | [`RecoveryMessage`](RecoveryMessage) | Provide state for sync | Any validator |
//!
//! ## View Change
//!
//! If the speaker fails or is Byzantine, validators trigger a view change:
//!
//! ```text
//! Validator detects timeout
//!           │
//!           ▼//!    Send ChangeView
//!           │
//!           ▼//!    Wait for M ChangeViews
//!           │
//!           ▼//!    New Speaker = validators[view % n]
//!           │
//!           ▼//!    Start new view
//! ```
//!
//! ## Change View Reasons
//!
//! | Reason | Description |
//! |--------|-------------|
//! | `Timeout` | Speaker didn't send PrepareRequest in time |
//! | `TxNotFound` | Transaction referenced in block not found |
//! | `TxRejectedByPolicy` | Transaction failed policy check |
//! | `TxInvalid` | Transaction failed verification |
//! | `BlockRejectedByPolicy` | Block failed policy check |
//! | `BlockInvalid` | Block verification failed |
//! | `ChangeAgreement` | Agreed to change view with other validators |
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use neo_consensus::{
//!     ConsensusService, ConsensusEvent, ConsensusCommand,
//!     ConsensusContext, ConsensusMessageType,
//! };
//! use neo_primitives::UInt160;
//! use tokio::sync::mpsc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create event channel for consensus notifications
//! let (event_tx, mut event_rx) = mpsc::channel(100);
//!
//! // Set up validator information
//! let validators = vec![
//!     UInt160::from_hex("...")?,  // Validator 0
//!     UInt160::from_hex("...")?,  // Validator 1
//!     UInt160::from_hex("...")?,  // Validator 2
//!     UInt160::from_hex("...")?,  // Validator 3
//! ];
//!
//! // Create consensus service
//! // If my_index is Some, we participate as a validator
//! let mut service = ConsensusService::new(
//!     0x4E454F,           // Network magic
//!     validators,          // Validator list (must be sorted)
//!     Some(0),            // Our validator index (or None for observer)
//!     private_key,        // Our signing key
//!     event_tx,           // Event sender
//! );
//!
//! // Start consensus for block 100
//! service.start(100, timestamp).await?;
//!
//! // Process incoming consensus messages
//! // (typically from P2P network)
//! service.process_message(consensus_payload).await?;
//!
//! // Handle consensus events
//! while let Some(event) = event_rx.recv().await {
//!     match event {
//!         ConsensusEvent::BlockCommitted { block, signatures } => {
//!             println!("Block {} committed with {} signatures!",
//!                 block.index, signatures.len());
//!         }
//!         ConsensusEvent::BroadcastMessage(payload) => {
//!             // Send to P2P network
//!             p2p.broadcast(payload).await?;
//!         }
//!         ConsensusEvent::ViewChanged { view, reason } => {
//!             println!("Changed to view {} due to {:?}", view, reason);
//!         }
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Configuration
//!
//! Timing parameters (configurable):
//!
//! | Parameter | Default | Description |
//! |-----------|---------|-------------|
//! | `BlockTime` | 15s | Target block interval |
//! | `PrepareRequestTimeout` | 4s | Wait for PrepareRequest |
//! | `PrepareResponseTimeout` | 4s | Wait for PrepareResponses |
//! | `CommitTimeout` | 4s | Wait for Commits |
//! | `ViewChangeTimeout` | 4s | Wait for view changes |
//!
//! ## Security Properties
//!
//! - **Safety**: No two honest nodes commit different blocks at the same height
//! - **Liveness**: If the network is synchronous and < 1/3 nodes are faulty, blocks are eventually committed
//! - **Accountability**: All consensus actions are signed and auditable

// ============================================================================
// Module Declarations
// ============================================================================

/// Reasons for requesting a view change.
pub mod change_view_reason;

/// Consensus state context.
///
/// Tracks the current view, validator set, signatures, and block data.
pub mod context;

/// Error types for consensus operations.
pub mod error;

/// Consensus message type enumeration.
pub mod message_type;

/// Consensus message types.
///
/// Contains all message types: PrepareRequest, PrepareResponse, Commit,
/// ChangeView, RecoveryRequest, and RecoveryMessage.
pub mod messages;

/// Main consensus service implementation.
///
/// The [`ConsensusService`] is the main state machine implementing dBFT 2.0.
pub mod service;

/// Consensus signer for message signing.
pub mod signer;

// ============================================================================
// Public Re-exports - Types
// ============================================================================

pub use change_view_reason::ChangeViewReason;
pub use error::{ConsensusError, ConsensusResult};
pub use message_type::ConsensusMessageType;
pub use signer::ConsensusSigner;

// ============================================================================
// Public Re-exports - Context
// ============================================================================

pub use context::{
    ConsensusContext, ConsensusState, ValidatorInfo, BLOCK_TIME_MS, MAX_VALIDATORS,
};

// ============================================================================
// Public Re-exports - Messages
// ============================================================================

pub use messages::{
    ChangeViewMessage, CommitMessage, ConsensusMessage, ConsensusPayload, PrepareRequestMessage,
    PrepareResponseMessage, RecoveryMessage, RecoveryRequestMessage,
};

// ============================================================================
// Public Re-exports - Service
// ============================================================================

pub use service::{BlockData, ConsensusCommand, ConsensusEvent, ConsensusService};
