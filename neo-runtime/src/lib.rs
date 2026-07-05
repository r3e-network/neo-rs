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
//! - `service`: Service loops, handles, import queues, lifecycle helpers, and
//!   command processing.

#![doc(html_root_url = "https://docs.rs/neo-runtime/0.10.0")]

mod errors;
pub mod node;
mod service;
pub mod time;

// Re-exports for the public surface of the crate.
//
// Everything the spec promises at the top level of `neo_runtime` is
// exported here so the docstring "use neo_runtime::NetworkService"
// import path resolves.
pub use errors::{ServiceError, ServiceResult, error};
pub use node::{ConfigProvider, NeoNodeTypes, NodeTypes, StoreProvider, TxAdmission};
pub use service::{
    BlockBatchImportOutcome, BlockImport, BlockImportOutcome, BlockImportQueue, BlockOrigin,
    BlockchainEvent, CommitPolicy, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY,
    ExecutionOutcome, ExecutionPayload, ImportQueue, ImportedTip, InMemorySyncStageCheckpointStore,
    Nep17Metadata, Nep17MetadataReader, NetworkEvent, NetworkService, Service, ServiceRegistry,
    StageProgress, SyncBlockBatch, SyncPipelineDriver, SyncPipelineImportOutcome,
    SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind, TxHash, ValidationResult,
};
pub use service::{
    block_import, blockchain, nep17, outcome, service_registry, services, sync_metrics,
    sync_pipeline,
};
