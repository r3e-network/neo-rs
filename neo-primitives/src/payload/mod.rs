//! # neo-primitives::payload
//!
//! Payload-domain primitives shared by protocol and network crates.
//!
//! ## Boundary
//!
//! This module belongs to `neo-primitives`. This foundation crate must stay
//! free of node-service, storage-backend, RPC, and network orchestration
//! dependencies.
//!
//! ## Contents
//!
//! - `inventory`: inventory payload traits and records.
//! - `log_event_args`: log event argument records.
//! - `serializable_payload`: serializable payload trait helpers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `verifiable`: verifiable payload trait helpers.
//! - `verification`: validation verdicts and verification coverage.

pub mod inventory;
pub mod log_event_args;
pub mod serializable_payload;
pub mod storage;
pub mod verifiable;
pub mod verification;
