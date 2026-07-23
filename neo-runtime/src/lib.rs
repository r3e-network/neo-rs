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
//! - `node`: Narrow storage and transaction-admission capabilities for
//!   upper-layer services.
//! - `service`: Service loops, handles, import queues, durable staged-sync
//!   providers, lifecycle helpers, and command processing.
//! - `support`: Protocol-neutral runtime measurement helpers.

#![doc(html_root_url = "https://docs.rs/neo-runtime/0.11.1")]

mod errors;
pub mod node;
mod service;
mod support;

// Re-exports for the public surface of the crate.
//
// Everything the spec promises at the top level of `neo_runtime` is
// exported here so the docstring "use neo_runtime::NetworkService"
// import path resolves.
pub use errors::{ServiceError, ServiceResult, error};
pub use node::{StoreProvider, TxAdmission};
pub use service::{
    BlockBatchImportOutcome, BlockCheckRejection, BlockImport, BlockImportOutcome,
    BlockImportQueue, BlockOrigin, BlockchainEvent, CheckedBlockBatch, CommitPolicy,
    CommittedHandler, CommittingHandler, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY,
    ExecutionOutcome, ExecutionPayload, FinalizedHandler, HeaderStageWindow, ImportQueue,
    ImportedTip, InMemorySyncStageCheckpointStore, InMemoryVerifiedHeaderStore,
    MAX_VERIFIED_HEADER_WINDOW, Nep17Metadata, Nep17MetadataReader, NetworkEvent, NetworkService,
    Service, SharedStoreSyncStageCheckpointStore, SharedStoreVerifiedHeaderStore, StageProgress,
    StoreSyncStageCheckpointStore, StoreVerifiedHeaderStore, SyncBlockBatch, SyncPipelineDriver,
    SyncPipelineImportOutcome, SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind,
    TxHash, ValidationResult, VerifiedHeaderStore, WalletChangedHandler,
};
pub use service::{
    block_import, blockchain, lifecycle, nep17, outcome, services, sync_metrics, sync_pipeline,
};
pub use support::time;
