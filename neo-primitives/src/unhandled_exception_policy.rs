//! Shared unhandled exception policy for plugin-like services.

use serde::Deserialize;
use std::any::Any;
use std::str::FromStr;

/// Error returned when parsing an unknown unhandled-exception policy name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseUnhandledExceptionPolicyError;

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

impl UnhandledExceptionPolicy {
    /// Parses a policy name using the case-insensitive names used by Neo plugin
    /// configuration files.
    pub fn from_name(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ignore" => Some(Self::Ignore),
            "stopplugin" => Some(Self::StopPlugin),
            "stopnode" => Some(Self::StopNode),
            "continue" => Some(Self::Continue),
            "terminate" => Some(Self::Terminate),
            _ => None,
        }
    }

    /// Applies the process/service-level effect of this policy.
    ///
    /// Returns `true` when the caller may continue processing. `StopPlugin`
    /// invokes `stop_plugin` and returns `false`; process-wide policies do not
    /// return.
    pub fn apply(self, stop_plugin: impl FnOnce()) -> bool {
        match self {
            Self::Ignore | Self::Continue => true,
            Self::StopPlugin => {
                stop_plugin();
                false
            }
            Self::StopNode => std::process::exit(1),
            Self::Terminate => std::process::abort(),
        }
    }
}

impl FromStr for UnhandledExceptionPolicy {
    type Err = ParseUnhandledExceptionPolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_name(value).ok_or(ParseUnhandledExceptionPolicyError)
    }
}

/// Extracts a loggable message from a panic payload while preserving caller
/// specific fallback wording for non-string payloads.
pub fn panic_message(payload: &(dyn Any + Send), fallback: &'static str) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        fallback.to_string()
    }
}

#[cfg(test)]
#[path = "tests/unhandled_exception_policy.rs"]
mod tests;
