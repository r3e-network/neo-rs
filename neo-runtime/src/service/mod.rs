//! # neo-runtime::service
//!
//! Service loops, handles, import queues, lifecycle helpers, and command
//! processing.
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
//! - `nep17`: Shared NEP-17 metadata capability contract.
//! - `outcome`: Runtime outcome records shared across services.
//! - `services`: Auxiliary service startup and handles used by the daemon.
//! - `sync_pipeline`: Shared staged-sync batch, checkpoint, commit-policy, and
//!   verified-header window primitives, with store-backed adapters isolated in
//!   child implementation modules.
//! - `sync_metrics`: Sync-speed counters, summaries, and operator-facing
//!   throughput status.

pub mod block_import;
pub mod blockchain;
pub mod nep17;
pub mod outcome;
pub mod services;
pub mod sync_metrics;
pub mod sync_pipeline;

pub use block_import::{
    BlockBatchImportOutcome, BlockCheckRejection, BlockImport, BlockImportOutcome,
    BlockImportQueue, BlockOrigin, CheckedBlockBatch, ImportQueue, ImportedTip,
};
pub use blockchain::{BlockchainEvent, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY};
pub use nep17::{Nep17Metadata, Nep17MetadataReader};
pub use outcome::{ExecutionOutcome, ExecutionPayload, NetworkEvent, ValidationResult};
pub use services::{NetworkService, Service, TxHash};
pub use sync_pipeline::{
    CommitPolicy, HeaderStageWindow, InMemorySyncStageCheckpointStore, InMemoryVerifiedHeaderStore,
    MAX_VERIFIED_HEADER_WINDOW, SharedStoreSyncStageCheckpointStore,
    SharedStoreVerifiedHeaderStore, StageProgress, StoreSyncStageCheckpointStore,
    StoreVerifiedHeaderStore, SyncBlockBatch, SyncPipelineDriver, SyncPipelineImportOutcome,
    SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind, VerifiedHeaderStore,
};
