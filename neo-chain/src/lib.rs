//! # Neo Chain
//!
//! Standalone blockchain state machine and chain management for Neo N3.
//!
//! **IMPORTANT**: This crate provides a **standalone chain state machine** without neo-core dependency.
//! For the full C# parity implementation with actor system and NeoSystem integration,
//! use `neo_core::ledger::blockchain` instead.
//!
//! ## When to use this crate
//!
//! - **Standalone tools**: Chain state management without full neo-core
//! - **Testing**: Block validation and fork choice logic in isolation
//! - **Alternative implementations**: Custom chain orchestration strategies
//! - **Lightweight clients**: Chain state without smart contract execution
//!
//! ## When to use neo-core::ledger::blockchain
//!
//! - **Full node operation**: C# parity with actor-based block processing
//! - **Plugin integration**: OnPersist/OnCommit events for plugins
//! - **P2P relay**: Block relay to connected peers
//! - **MemoryPool integration**: Transaction verification context
//!
//! ## Features
//!
//! - Chain state management
//! - Block validation and processing
//! - Fork choice rules
//! - Chain reorganization handling
//! - Block indexing and queries
//!
//! ## Architecture
//!
//! The chain module is the central coordinator for blockchain state:
//! - Receives blocks from consensus or P2P
//! - Validates blocks against protocol rules
//! - Updates persistent state
//! - Handles chain reorganizations
//! - Notifies subscribers of state changes

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
