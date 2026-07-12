//! # neo-system::composition::sync
//!
//! Node-composed header, body, import, and live-inventory workflows.
//!
//! ## Boundary
//!
//! This module wires reusable runtime and network capabilities over the one
//! canonical blockchain handle. It does not own protocol validation,
//! persistence, peer transport, or application task supervision.
//!
//! ## Contents
//!
//! - `live_block_import_pipeline`: lossy unsolicited-inventory preflight.
//! - `staged_sync_pipeline`: typed `Headers -> Bodies -> Import` facade.
//! - `sync_download_import`: downloaded-batch to import-stage bridge.
//! - `sync_header_pipeline`: durable verified-header stage and body gate.
//! - `sync_import_pipeline`: bounded import queue and checkpoint wiring.
//! - `verified_block_fetcher`: body/header agreement adapter.

pub mod live_block_import_pipeline;
pub mod staged_sync_pipeline;
pub mod sync_download_import;
pub mod sync_header_pipeline;
pub mod sync_import_pipeline;
pub mod verified_block_fetcher;

pub use live_block_import_pipeline::{LiveBlockImportPipeline, LiveBlockImportSummary};
pub use staged_sync_pipeline::StagedSyncPipeline;
pub use sync_download_import::{SyncDownloadImportDriver, SyncDownloadImportSummary};
pub use sync_header_pipeline::{HeaderStageBatchOutcome, HeaderStageProgress, SyncHeaderPipeline};
pub use sync_import_pipeline::SyncImportPipeline;
pub use verified_block_fetcher::VerifiedBlockRangeFetcher;
