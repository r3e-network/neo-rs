//! Application logs plugin support (parity with Neo.Plugins.ApplicationLogs).

mod service;
mod settings;

pub use service::ApplicationLogsService;
pub use settings::ApplicationLogsSettings;
