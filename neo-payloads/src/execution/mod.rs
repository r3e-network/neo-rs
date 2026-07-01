//! # neo-payloads::execution
//!
//! Execution payload records and VM-result domain types.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `application_executed`: application execution event records.
//! - `event_handlers`: execution event handler records.
//! - `notify_event_args`: contract notification event records.

/// Per-transaction execution record emitted when a block is processed.
pub mod application_executed;
/// Event payloads and handler traits used by Neo plugins and services.
pub mod event_handlers;
/// Contract notification event arguments.
pub mod notify_event_args;
