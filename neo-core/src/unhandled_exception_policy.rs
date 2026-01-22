//! Shared unhandled exception policy for plugin-like services.

use serde::Deserialize;

/// Exception handling policy for plugin-style services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum UnhandledExceptionPolicy {
    /// Ignore exceptions and continue processing.
    Ignore,
    /// Stop the plugin/service on exception.
    StopPlugin,
    /// Stop the node on exception.
    #[default]
    StopNode,
    /// Continue processing after logging exception.
    Continue,
    /// Terminate the process immediately.
    Terminate,
}
