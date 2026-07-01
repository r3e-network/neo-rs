//! # neo-indexer
//!
//! Chain indexer service, schema models, and durable indexed-data store.
//!
//! ## Boundary
//!
//! This service crate owns projections over committed chain data and must not
//! decide block validity or consensus outcomes.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.
//! - `indexer`: Indexer workers and projection logic for chain-derived data.
//! - `model`: indexer snapshot and projection model records.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `store`: Store implementation for the surrounding backend or domain.
//! - `tests`: Module-local tests and regression coverage.

#[path = "errors/error.rs"]
mod error;
mod indexer;
#[path = "schema/model.rs"]
mod model;
mod service;
mod store;

pub use error::{IndexerError, IndexerResult};
pub use indexer::Indexer;
pub use model::{
    AccountTransactionRecord, BlockIndexRecord, INDEXER_SNAPSHOT_VERSION, IndexerSnapshot,
    IndexerStatus, NotificationIndexRecord, TransactionIndexRecord,
};
pub use service::IndexerService;

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
