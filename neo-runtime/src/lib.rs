//! # neo-runtime
//!
//! Shared runtime service traits, events, handles, and block-import contracts.
//!
//! ## Boundary
//!
//! This runtime API crate owns shared service contracts and must not depend on
//! concrete node binaries or UI composition.
//!
//! ## Contents
//!
//! - `errors`: Typed errors and result aliases for this crate boundary.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.

#![doc(html_root_url = "https://docs.rs/neo-runtime/0.9.0")]

mod errors;
mod service;

// Re-exports for the public surface of the crate.
//
// Everything the spec promises at the top level of `neo_runtime` is
// exported here so the docstring "use neo_runtime::BlockExecutor"
// import path resolves.
pub use errors::{ServiceError, ServiceResult, error};
pub use service::{
    BlockBatchImportOutcome, BlockExecutor, BlockImport, BlockImportOutcome, BlockOrigin,
    BlockchainEvent, ConsensusService, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY,
    ExecutionOutcome, ExecutionPayload, ImportedTip, NeoEngine, NetworkEvent, NetworkService,
    Service, TxHash, ValidationResult,
};
pub use service::{block_import, blockchain, outcome, services, sync_metrics};
