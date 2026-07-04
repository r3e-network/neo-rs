//! TokensTracker runtime.
//!
//! Provides balance/transfer indexing for NEP-17/NEP-11 standards. This struct is
//! registered as a committing/committed handler to process block events.

use super::settings::TokensTrackerSettings;
use super::trackers::nep_11::Nep11Tracker;
use super::trackers::nep_17::Nep17Tracker;
use super::trackers::tracker_base::Tracker;
use neo_config::ProtocolSettings;
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_payloads::{CommittedHandler, CommittingHandler};
use neo_primitives::panic_message;
use neo_storage::persistence::{DataCache, Store};
use parking_lot::RwLock;
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::error;

/// Runtime handler for token balance/transfer tracking.
///
/// Implements `CommittingHandler` and `CommittedHandler` to index
/// token transfers during block commits.
pub struct TokensTracker {
    settings: TokensTrackerSettings,
    trackers: RwLock<Vec<Box<dyn Tracker>>>,
    disabled: AtomicBool,
}

impl TokensTracker {
    /// Creates a new TokensTracker with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `settings` - Tracker configuration
    /// * `db` - Database store for balance/transfer data
    /// * `protocol_settings` - Protocol settings (for VM execution)
    pub fn new(settings: TokensTrackerSettings, db: Arc<dyn Store>, protocol_settings: Arc<ProtocolSettings>) -> Self {
        let mut trackers: Vec<Box<dyn Tracker>> = Vec::new();

        if settings.enabled_nep17() {
            trackers.push(Box::new(Nep17Tracker::new(
                Arc::clone(&db),
                settings.max_results,
                settings.track_history,
                Arc::clone(&protocol_settings),
            )));
        }

        if settings.enabled_nep11() {
            trackers.push(Box::new(Nep11Tracker::new(
                Arc::clone(&db),
                settings.max_results,
                settings.track_history,
                Arc::clone(&protocol_settings),
            )));
        }

        Self {
            settings,
            trackers: RwLock::new(trackers),
            disabled: AtomicBool::new(false),
        }
    }

    /// Returns a reference to the settings.
    pub fn settings(&self) -> &TokensTrackerSettings {
        &self.settings
    }

    fn handle_panic(&self, tracker: &str, action: &str, payload: Box<dyn Any + Send>) -> bool {
        let message = panic_message(payload.as_ref(), "panic");
        self.handle_failure(tracker, action, "panicked", message)
    }

    fn handle_error(&self, tracker: &str, action: &str, error_message: String) -> bool {
        self.handle_failure(tracker, action, "failed", error_message)
    }

    fn handle_failure(
        &self,
        tracker: &str,
        action: &str,
        outcome: &'static str,
        error_message: String,
    ) -> bool {
        match self.settings.exception_policy {
            neo_primitives::unhandled_exception_policy::UnhandledExceptionPolicy::Ignore => {
                return true;
            }
            _ => {
                error!(
                    target: "neo::tokens_tracker",
                    track = tracker,
                    action,
                    error = error_message,
                    "tokens tracker {outcome}"
                );
            }
        }

        self.apply_exception_policy()
    }

    fn apply_exception_policy(&self) -> bool {
        self.settings
            .exception_policy
            .apply(|| self.disabled.store(true, Ordering::Relaxed))
    }

    fn run_tracker_action<F>(&self, tracker: &str, action: &str, f: F) -> bool
    where
        F: FnOnce(),
    {
        match panic::catch_unwind(AssertUnwindSafe(f)) {
            Ok(()) => true,
            Err(payload) => self.handle_panic(tracker, action, payload),
        }
    }

    fn run_tracker_result_action<F>(&self, tracker: &str, action: &str, f: F) -> bool
    where
        F: FnOnce() -> neo_error::CoreResult<()>,
    {
        match panic::catch_unwind(AssertUnwindSafe(f)) {
            Ok(Ok(())) => true,
            Ok(Err(err)) => self.handle_error(tracker, action, err.to_string()),
            Err(payload) => self.handle_panic(tracker, action, payload),
        }
    }
}

impl CommittingHandler for TokensTracker {
    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        let Some(settings) = system.downcast_ref::<ProtocolSettings>() else {
            return;
        };
        if settings.network != self.settings.network {
            return;
        }

        if self.disabled.load(Ordering::Relaxed) {
            return;
        }

        let mut trackers = self.trackers.write();
        for tracker in trackers.iter_mut() {
            if self.disabled.load(Ordering::Relaxed) {
                break;
            }
            let track_name = tracker.track_name().to_string();
            if !self.run_tracker_action(&track_name, "reset_batch", || tracker.reset_batch()) {
                break;
            }
            if !self.run_tracker_action(&track_name, "on_persist", || {
                tracker.on_persist(block, snapshot, application_executed_list)
            }) {
                break;
            }
        }
    }
}

impl CommittedHandler for TokensTracker {
    fn blockchain_committed_handler(&self, system: &dyn Any, _block: &Block) {
        let Some(settings) = system.downcast_ref::<ProtocolSettings>() else {
            return;
        };
        if settings.network != self.settings.network {
            return;
        }

        if self.disabled.load(Ordering::Relaxed) {
            return;
        }

        let mut trackers = self.trackers.write();
        for tracker in trackers.iter_mut() {
            if self.disabled.load(Ordering::Relaxed) {
                break;
            }
            let track_name = tracker.track_name().to_string();
            if !self.run_tracker_result_action(&track_name, "commit", || tracker.commit()) {
                break;
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/plugins/tokens_tracker/runtime.rs"]
mod tests;
