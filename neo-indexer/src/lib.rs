//! Read-side indexer service for Neo N3 nodes.
//!
//! The crate indexes canonical block imports into query-friendly block,
//! transaction, and signer-account records. It intentionally stays outside the
//! consensus and block-validation path: callers feed it blocks after persistence
//! and use the service facade from RPC/REST/front-end integrations.

mod error;
mod indexer;
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
