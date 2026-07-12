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
//! - `staged_sync_pipeline`: typed `Headers -> Bodies -> Import` entry point.
//! - `sync_download_import`: Download-stream to sync-import bridge.
//! - `sync_header_pipeline`: durable verified-header stage and body gate.
//! - `sync_import_pipeline`: Node-local sync import queue/checkpoint wiring.
//! - `tx_admission_provider`: Ledger/native read seams for transaction
//!   admission routing.
//! - `wallet_provider`: wallet provider adapter.

pub mod builder;
pub mod core;
pub mod node;
pub mod staged_sync_pipeline;
pub mod sync_download_import;
pub mod sync_header_pipeline;
pub mod sync_import_pipeline;
pub mod system_context;
mod tx_admission_provider;
pub mod verified_block_fetcher;
pub mod wallet_provider;

pub use builder::NodeBuilder;
pub use core::{BlockchainTask, NodeCore, NodeCoreBuilder, NodeCoreLaunch};
pub use node::Node;
pub use staged_sync_pipeline::StagedSyncPipeline;
pub use sync_download_import::{SyncDownloadImportDriver, SyncDownloadImportSummary};
pub use sync_header_pipeline::{HeaderStageBatchOutcome, HeaderStageProgress, SyncHeaderPipeline};
pub use sync_import_pipeline::SyncImportPipeline;
pub use system_context::{BlockCommitHooks, NodeSystemContext, NoopBlockCommitHooks};
pub use verified_block_fetcher::VerifiedBlockRangeFetcher;
pub use wallet_provider::WalletProvider;
