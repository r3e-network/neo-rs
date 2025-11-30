//! # Neo Consensus
//!
//! Delegated Byzantine Fault Tolerance (dBFT) consensus for the Neo blockchain.
//!
//! This crate implements the dBFT 2.0 consensus algorithm used by Neo N3.
//!
//! ## Algorithm Overview
//!
//! dBFT is a Byzantine Fault Tolerant consensus mechanism that:
//! - Achieves finality in a single block (no forks)
//! - Tolerates f = (n-1)/3 Byzantine nodes
//! - Uses a rotating speaker/validator model
//!
//! ## Core Types
//!
//! - [`ConsensusMessageType`]: Types of consensus messages (PrepareRequest, Commit, etc.)
//! - [`ChangeViewReason`]: Reasons for requesting a view change
//!
//! ## Consensus Flow
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    dBFT Consensus Flow                       │
//! │                                                              │
//! │  Speaker                    Validators                       │
//! │    │                           │                             │
//! │    │──── PrepareRequest ──────>│                             │
//! │    │                           │                             │
//! │    │<─── PrepareResponse ──────│                             │
//! │    │                           │                             │
//! │    │<──────── Commit ──────────│                             │
//! │    │                           │                             │
//! │    │         Block Committed   │                             │
//! │    ▼                           ▼                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust
//! use neo_consensus::{ConsensusMessageType, ChangeViewReason};
//!
//! // Parse message type from byte
//! let msg_type = ConsensusMessageType::from_byte(0x20);
//! assert_eq!(msg_type, Some(ConsensusMessageType::PrepareRequest));
//!
//! // Check change view reason
//! let reason = ChangeViewReason::Timeout;
//! assert_eq!(reason.to_byte(), 0x00);
//! ```

pub mod change_view_reason;
pub mod error;
pub mod message_type;

// Re-exports
pub use change_view_reason::ChangeViewReason;
pub use error::{ConsensusError, ConsensusResult};
pub use message_type::ConsensusMessageType;

// Placeholder for future modules
// pub mod service;
// pub mod context;
// pub mod messages;
