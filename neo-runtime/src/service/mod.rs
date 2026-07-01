//! # neo-runtime::service
//!
//! Service loops, handles, lifecycle helpers, and command processing.
//!
//! ## Boundary
//!
//! This module belongs to `neo-runtime`. This runtime API crate owns shared
//! service contracts and must not depend on concrete node binaries or UI
//! composition.
//!
//! ## Contents
//!
//! - `block_import`: Shared block-import trait and outcome records.
//! - `blockchain`: Blockchain-domain primitive records used across crates.
//! - `outcome`: Runtime outcome records shared across services.
//! - `services`: Auxiliary service startup and handles used by the daemon.
//! - `sync_metrics`: Sync-speed counters, summaries, and operator-facing
//!   throughput status.

pub mod block_import;
pub mod blockchain;
pub mod outcome;
pub mod services;
pub mod sync_metrics;

pub use block_import::{
    BlockBatchImportOutcome, BlockImport, BlockImportOutcome, BlockOrigin, ImportedTip,
};
pub use blockchain::{BlockchainEvent, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY};
pub use outcome::{ExecutionOutcome, ExecutionPayload, NetworkEvent, ValidationResult};
pub use services::{BlockExecutor, ConsensusService, NeoEngine, NetworkService, Service, TxHash};
