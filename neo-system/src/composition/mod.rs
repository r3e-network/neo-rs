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
//! - `sync_download_import`: Download-stream to sync-import bridge.
//! - `sync_import_pipeline`: Node-local sync import queue/checkpoint wiring.
//! - `tx_admission_provider`: Ledger/native read seams for transaction
//!   admission routing.
//! - `wallet_provider`: wallet provider adapter.

pub mod builder;
pub mod core;
pub mod node;
pub mod sync_download_import;
pub mod sync_import_pipeline;
pub mod system_context;
mod tx_admission_provider;
pub mod wallet_provider;

pub use builder::NodeBuilder;
pub use core::{BlockchainTask, NodeCore, NodeCoreBuilder, NodeCoreLaunch};
pub use node::Node;
pub use sync_download_import::{SyncDownloadImportDriver, SyncDownloadImportSummary};
pub use sync_import_pipeline::SyncImportPipeline;
pub use system_context::{BlockCommitHooks, NodeSystemContext, NoopBlockCommitHooks};
pub use wallet_provider::WalletProvider;
