//! # neo-consensus::tests::service
//!
//! Test module grouping Service loops, handles, lifecycle helpers, and command
//! processing. coverage for neo-consensus.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-consensus; it may assemble fixtures
//! but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `helpers`: Shared helper functions for the surrounding module.
//! - `change_view`: dBFT ChangeView message records.
//! - `core`: Core reader, writer, var-int, and macro helpers for binary IO.
//! - `persist`: service persistence regression coverage.
//! - `prepare`: prepare-request and prepare-response coverage.
//! - `recovery`: dBFT recovery request and response messages.

#[path = "helpers.rs"]
mod helpers;

#[path = "change_view.rs"]
mod change_view;
#[path = "core.rs"]
mod core;
#[path = "persist.rs"]
mod persist;
#[path = "prepare.rs"]
mod prepare;
#[path = "recovery.rs"]
mod recovery;
