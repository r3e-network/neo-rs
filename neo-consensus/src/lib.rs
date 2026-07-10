// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # neo-consensus
//!
//! dBFT consensus messages, context, signer abstraction, and service logic.
//!
//! ## Boundary
//!
//! This protocol/service crate owns dBFT state and messages and must not own
//! ledger persistence, RPC transport, or application startup.
//!
//! ## Contents
//!
//! - `change_view_reason`: dBFT change-view reason codes.
//! - `context`: Runtime context records carried through the local workflow.
//! - `error`: Typed error definitions and conversions.
//! - `message_type`: consensus message type identifiers.
//! - `messages`: Typed service commands, events, and payload wrappers for the
//!   crate boundary.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `signer`: signer configuration and signing helpers.

// ============================================================================
// Module Declarations
// ============================================================================

/// Reasons for requesting a view change.
#[path = "protocol/change_view_reason.rs"]
pub mod change_view_reason;

/// Consensus state context.
///
/// Tracks the current view, validator set, signatures, and block data.
pub mod context;

/// Error types for consensus operations.
#[path = "errors/error.rs"]
pub mod error;

/// Consensus message type enumeration.
#[path = "protocol/message_type.rs"]
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
#[path = "protocol/signer.rs"]
pub mod signer;

// ============================================================================
// Public Re-exports - Types
// ============================================================================

pub use change_view_reason::ChangeViewReason;
pub use error::{ConsensusError, ConsensusResult};
pub use message_type::ConsensusMessageType;
pub use signer::{ConsensusSigner, NoConsensusSigner};

// ============================================================================
// Public Re-exports - Context
// ============================================================================

pub use context::{
    BLOCK_TIME_MS, ConsensusContext, ConsensusState, DEFAULT_BLOCK_TIME_MS, MAX_VALIDATORS,
    ValidatorInfo,
};

// ============================================================================
// Public Re-exports - Messages
// ============================================================================

pub use messages::{
    ChangeViewMessage, CommitMessage, ConsensusPayload, PrepareRequestMessage,
    PrepareResponseMessage, RecoveryMessage, RecoveryRequestMessage,
};

// ============================================================================
// Public Re-exports - Service
// ============================================================================

pub use service::{BlockData, ConsensusCommand, ConsensusEvent, ConsensusService};
