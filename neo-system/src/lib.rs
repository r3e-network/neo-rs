//! # neo-system
//!
//! Composition root for building node services, staged sync, wallets, and
//! runtime dependencies.
//!
//! ## Boundary
//!
//! This composition crate wires services and must not hide protocol rules or
//! duplicate lower-layer business logic.
//!
//! ## Contents
//!
//! - `composition`: Composition-root builders and node assembly
//!   helpers.
//! - `errors`: Typed errors and result aliases for this crate boundary.

#![doc(html_root_url = "https://docs.rs/neo-system/0.10.0")]

mod composition;
mod errors;

// Public re-exports for the crate's public surface.
pub use composition::{
    BlockCommitHooks, BlockchainTask, CanonicalCommit, DEFAULT_FINALITY_CAPACITY,
    FinalizedBlockConsumer, FinalizedBlockHandle, FinalizedBlockStream, FinalizedBlockStreamError,
    FinalizedBlockStreamFactory, HeaderStageBatchOutcome, HeaderStageProgress,
    LiveBlockImportPipeline, LiveBlockImportSummary, Node, NodeBuilder, NodeCore, NodeCoreBuilder,
    NodeCoreLaunch, NodeSystemContext, NoopBlockCommitHooks, StagedSyncPipeline,
    SyncDownloadImportDriver, SyncDownloadImportSummary, SyncHeaderPipeline, SyncImportPipeline,
    VerifiedBlockRangeFetcher, WalletProvider,
};
pub use composition::{
    builder, core, finality, live_block_import_pipeline, node, staged_sync_pipeline,
    sync_download_import, sync_header_pipeline, sync_import_pipeline, system_context,
    verified_block_fetcher, wallet_provider,
};
pub use errors::{NodeError, NodeResult, error};
