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

impl UnhandledExceptionPolicy {
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

#[cfg(test)]
mod tests {
    use super::UnhandledExceptionPolicy;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn continuing_policies_do_not_stop_plugin() {
        let stopped = AtomicBool::new(false);

        assert!(UnhandledExceptionPolicy::Ignore.apply(|| {
            stopped.store(true, Ordering::Relaxed);
        }));
        assert!(UnhandledExceptionPolicy::Continue.apply(|| {
            stopped.store(true, Ordering::Relaxed);
        }));

        assert!(!stopped.load(Ordering::Relaxed));
    }

    #[test]
    fn stop_plugin_invokes_callback_and_stops_processing() {
        let stopped = AtomicBool::new(false);

        let should_continue = UnhandledExceptionPolicy::StopPlugin.apply(|| {
            stopped.store(true, Ordering::Relaxed);
        });

        assert!(!should_continue);
        assert!(stopped.load(Ordering::Relaxed));
    }
}
