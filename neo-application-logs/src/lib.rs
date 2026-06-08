//! ApplicationLogs plugin support (parity with Neo.Plugins.ApplicationLogs).
//!
//! Captures per-block and per-transaction execution logs on the blockchain
//! commit hooks and serves them to RPC queries. Extracted from
//! `neo-core/src/application_logs` into a standalone leaf crate that depends on
//! `neo-core`; nothing in `neo-core` depends back on it.

mod service;
mod settings;

pub use service::ApplicationLogsService;
pub use settings::ApplicationLogsSettings;
