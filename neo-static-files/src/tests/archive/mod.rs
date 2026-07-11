//! # Static archive tests
//!
//! ## Boundary
//!
//! These tests exercise one archive file together with its derived MDBX
//! sidecar. They do not assign Neo-specific meaning to stored keys or values.
//!
//! ## Contents
//!
//! - `helpers`: Shared test records and corruption helpers.
//! - `index`: Persistent-index startup and crash-ordering behavior.
//! - `operations`: Append, lookup, truncation, and configuration contracts.
//! - `ownership`: Kernel writer-lease behavior across path aliases.
//! - `recovery`: Torn-tail and retained-corruption handling.

mod helpers;
mod index;
mod operations;
mod ownership;
mod recovery;
