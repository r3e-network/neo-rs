//! # neo-state-service::protocol
//!
//! Protocol enums, versioned records, and chain-level domain constants.
//!
//! ## Boundary
//!
//! This module belongs to `neo-state-service`. This service crate owns state-
//! root and MPT service behavior and must not own block download, consensus,
//! RPC transport, or UI composition.
//!
//! ## Contents
//!
//! - `keys`: state-root validator key records.
//! - `message_type`: state-service message type identifiers.
//! - `metrics`: Metrics collection and progress-reporting helpers.
//! - `state_root`: state-root records and version constants.
//! - `vote`: validator vote records for state roots.

pub mod keys;
pub mod message_type;
/// Lightweight state-root ingestion counters.
pub mod metrics;
pub mod state_root;
pub mod vote;

pub use keys::Keys;
pub use message_type::MessageType;
pub use metrics::{
    StateRootApplyMetrics, StateRootApplyStats, StateRootIngestMetrics, StateRootIngestStats,
};
pub use state_root::{CURRENT_VERSION, StateRoot};
pub use vote::Vote;
