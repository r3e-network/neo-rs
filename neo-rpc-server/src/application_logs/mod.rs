//! Application logs plugin support (parity with Neo.Plugins.ApplicationLogs).

mod service;
mod settings;

pub use service::{
    AcceptedFlush, ApplicationLogsService, DEFAULT_ACCEPTED_BYTE_CAP, ImportSessionOutcome,
    LogsFlushTelemetry, PersistenceUnitOutcome,
};
pub use settings::ApplicationLogsSettings;
