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
mod production_hygiene_tests {
    const RUNTIME_SOURCES: &[(&str, &str)] = &[
        ("indexer.rs", include_str!("indexer.rs")),
        ("service.rs", include_str!("service.rs")),
    ];

    #[test]
    fn indexer_runtime_sources_do_not_panic_on_recoverable_state() {
        for (name, source) in RUNTIME_SOURCES {
            let production = source.split("#[cfg(test)]").next().unwrap_or(source);
            for forbidden in [".expect(", ".unwrap(", "panic!", "todo!", "unimplemented!"] {
                assert!(
                    !production.contains(forbidden),
                    "{name} production path should return IndexerError instead of using {forbidden}"
                );
            }
        }
    }
}
