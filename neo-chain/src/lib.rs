//! # Neo Chain
//!
//! Standalone blockchain state machine and chain management for Neo N3.
//!
//! ## Features
//!
//! - Chain state management
//! - Block validation and processing
//! - Fork choice rules
//! - Chain reorganization handling
//! - Block indexing and queries

mod block_index;
mod chain_state;
mod error;
mod events;
mod fork_choice;
mod validation;

pub use block_index::{BlockIndex, BlockIndexEntry};
pub use chain_state::{ChainState, ChainStateSnapshot};
pub use error::{ChainError, ChainResult};
pub use events::{ChainEvent, ChainEventSubscriber};
pub use fork_choice::ForkChoice;
pub use validation::{BlockValidator, ValidationResult};

/// Genesis block height
pub const GENESIS_HEIGHT: u32 = 0;

/// Maximum allowed block time drift (in seconds)
pub const MAX_TIME_DRIFT_SECS: u64 = 15 * 60; // 15 minutes
