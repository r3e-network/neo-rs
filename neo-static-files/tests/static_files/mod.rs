//! # neo-static-files::tests::static_files
//!
//! Static-file integration tests for append, read, fsync, and recovery
//! behavior.
//!
//! ## Boundary
//!
//! This is test-only code for `neo-static-files`; it may create temporary
//! segment files but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `append_store`: append-store contract tests for offsets and truncation.

mod append_store;
