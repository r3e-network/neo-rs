//! Application Logs Plugin - Main module
//!
//! This module provides the Application Logs plugin implementation
//! matching the C# Neo.Plugins.ApplicationLogs exactly.

pub mod application_logs_plugin;
pub mod log_reader;
pub mod rpc_handlers;
pub mod settings;
pub mod store;

// Re-export commonly used types
pub use application_logs_plugin::ApplicationLogsPlugin;
pub use log_reader::LogReader;
pub use rpc_handlers::{register_log_reader, unregister_log_reader, ApplicationLogsRpcHandlers};
pub use settings::ApplicationLogsSettings;
