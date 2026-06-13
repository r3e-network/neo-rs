//! ApplicationLogs plugin support (parity with Neo.Plugins.ApplicationLogs).
//!
//! Captures per-block and per-transaction execution logs on the blockchain
//! commit hooks and serves them to RPC queries.

mod service;
mod settings;

pub use service::ApplicationLogsService;
pub use settings::ApplicationLogsSettings;
