//! # neo-rpc::application_logs
//!
//! Application-log models and retrieval helpers for RPC consumers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `rendering`: Application-log rendering helpers.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `settings`: Protocol settings, hardfork gates, and node configuration
//!   records.
//! - `stack_json`: Stack-item JSON rendering helpers.

mod rendering;
mod service;
mod settings;
mod stack_json;

pub use service::ApplicationLogsService;
pub use settings::ApplicationLogsSettings;
