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
//! - `import_queue`: Bounded preverification queue for sync/import callers.
//! - `outcome`: Runtime outcome records shared across services.
//! - `service_registry`: Type-map registry for optional node services.
//! - `services`: Auxiliary service startup and handles used by the daemon.
//! - `sync_pipeline`: Staged-sync commit policies and checkpoint traits.
//! - `sync_metrics`: Sync-speed counters, summaries, and operator-facing
//!   throughput status.

pub mod block_import;
pub mod blockchain;
pub mod import_queue;
pub mod nep17;
pub mod outcome;
pub mod service_registry;
pub mod services;
pub mod sync_metrics;
pub mod sync_pipeline;

pub use block_import::{
    BlockBatchImportOutcome, BlockImport, BlockImportOutcome, BlockOrigin, ImportedTip,
};
pub use blockchain::{BlockchainEvent, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY};
pub use import_queue::{BlockImportQueue, ImportQueue};
pub use nep17::{Nep17Metadata, Nep17MetadataReader};
pub use outcome::{ExecutionOutcome, ExecutionPayload, NetworkEvent, ValidationResult};
pub use service_registry::ServiceRegistry;
pub use services::{NetworkService, Service, TxHash};
pub use sync_pipeline::{
    CommitDecision, CommitPolicy, CommitTrigger, InMemorySyncStageCheckpointStore, StageProgress,
    SyncBlockBatch, SyncPipelineDriver, SyncPipelineImportOutcome, SyncStageCheckpoint,
    SyncStageCheckpointStore, SyncStageKind,
};
