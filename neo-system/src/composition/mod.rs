//! # neo-system::composition
//!
//! Composition-root builders and node assembly helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-system`. This composition crate wires services
//! and must not hide protocol rules or duplicate lower-layer business logic.
//!
//! ## Contents
//!
//! - `builder`: final composed-node builder.
//! - `core`: provider-neutral core service construction and staged launch.
//! - `node`: composed node runtime and capability accessors.
//! - `sync`: header, body, import, and live-inventory workflows.
//! - `wallet_provider`: wallet provider adapter.

pub mod builder;
pub mod core;
pub mod finality;
pub mod node;
pub mod sync;
pub mod system_context;
pub mod wallet_provider;

pub use builder::NodeBuilder;
pub use core::{BlockchainTask, NodeCore, NodeCoreBuilder, NodeCoreLaunch};
pub use finality::{
    DEFAULT_FINALITY_CAPACITY, FinalizedBlockConsumer, FinalizedBlockHandle, FinalizedBlockStream,
    FinalizedBlockStreamError, FinalizedBlockStreamFactory,
};
pub use node::Node;
pub use sync::{
    HeaderStageBatchOutcome, HeaderStageProgress, LiveBlockImportPipeline, LiveBlockImportSummary,
    StagedSyncPipeline, SyncDownloadImportDriver, SyncDownloadImportSummary, SyncHeaderPipeline,
    SyncImportPipeline, VerifiedBlockRangeFetcher, live_block_import_pipeline,
    staged_sync_pipeline, sync_download_import, sync_header_pipeline, sync_import_pipeline,
    verified_block_fetcher,
};
pub use system_context::{
    BlockCommitHooks, CanonicalCommit, NodeSystemContext, NoopBlockCommitHooks,
};
pub use wallet_provider::WalletProvider;
