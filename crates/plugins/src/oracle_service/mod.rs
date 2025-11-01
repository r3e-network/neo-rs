//! Oracle Service Plugin
//!
//! This plugin provides oracle functionality for the Neo blockchain,
//! matching the C# Neo.Plugins.OracleService exactly.

pub mod helper;
pub mod oracle_service;
pub mod oracle_service_plugin;
pub mod protocols;
pub mod settings;

// Re-export commonly used types
pub use helper::OracleServiceHelper;
pub use oracle_service::OracleService;
pub use oracle_service_plugin::OracleServicePlugin;
pub use settings::OracleServiceSettings;
