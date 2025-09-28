//! Application Logs Plugin - Main module
//!
//! This module provides the Application Logs plugin implementation
//! matching the C# Neo.Plugins.ApplicationLogs exactly.

pub mod application_logs_plugin;
pub mod log_reader;
pub mod settings;
pub mod store;

// Re-export commonly used types
pub use application_logs_plugin::ApplicationLogsPlugin;
pub use log_reader::LogReader;
pub use settings::ApplicationLogsSettings;