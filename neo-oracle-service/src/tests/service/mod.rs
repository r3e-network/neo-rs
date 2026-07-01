//! # neo-oracle-service::tests::service
//!
//! Test module grouping Service loops, handles, lifecycle helpers, and command
//! processing. coverage for neo-oracle-service.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-oracle-service; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `dedup`: oracle request deduplication coverage.
//! - `filter_json`: oracle JSON filtering coverage.
//! - `response_tx`: oracle response transaction coverage.

#[path = "dedup.rs"]
mod dedup;
#[path = "filter_json.rs"]
mod filter_json;
#[path = "response_tx.rs"]
mod response_tx;
