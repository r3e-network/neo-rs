//! TokensTracker runtime.
//!
//! Provides balance/transfer indexing for NEP-17/NEP-11 standards. This struct is
//! registered as a committing/committed handler to process block events.

use super::settings::TokensTrackerSettings;
use super::trackers::nep_11::Nep11Tracker;
use super::trackers::nep_17::Nep17Tracker;
use super::trackers::tracker_base::Tracker;
use crate::i_event_handlers::{ICommittedHandler, ICommittingHandler};
use crate::neo_ledger::{ApplicationExecuted, Block};
use crate::persistence::{DataCache, IStore};
use crate::NeoSystem;
use parking_lot::RwLock;
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::error;

/// Runtime handler for token balance/transfer tracking.
///
/// Implements `ICommittingHandler` and `ICommittedHandler` to index
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
    /// * `neo_system` - Reference to the Neo system
    pub fn new(
        settings: TokensTrackerSettings,
        db: Arc<dyn IStore>,
        neo_system: Arc<NeoSystem>,
    ) -> Self {
        let mut trackers: Vec<Box<dyn Tracker>> = Vec::new();

        if settings.enabled_nep17() {
            trackers.push(Box::new(Nep17Tracker::new(
                Arc::clone(&db),
                settings.max_results,
                settings.track_history,
                Arc::clone(&neo_system),
            )));
        }

        if settings.enabled_nep11() {
            trackers.push(Box::new(Nep11Tracker::new(
                Arc::clone(&db),
                settings.max_results,
                settings.track_history,
                Arc::clone(&neo_system),
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

    fn handle_panic(
        &self,
        tracker: &str,
        action: &str,
        payload: Box<dyn Any + Send>,
    ) -> bool {
        let message = panic_message(&payload);
        use crate::unhandled_exception_policy::UnhandledExceptionPolicy;
        match self.settings.exception_policy {
            UnhandledExceptionPolicy::Ignore => return true,
            _ => {
                error!(
                    target: "neo::tokens_tracker",
                    track = tracker,
                    action = action,
                    error = message,
                    "tokens tracker panicked"
                );
            }
        }

        match self.settings.exception_policy {
            UnhandledExceptionPolicy::Ignore => true,
            UnhandledExceptionPolicy::Continue => true,
            UnhandledExceptionPolicy::StopPlugin => {
                self.disabled.store(true, Ordering::Relaxed);
                false
            }
            UnhandledExceptionPolicy::StopNode => std::process::exit(1),
            UnhandledExceptionPolicy::Terminate => std::process::abort(),
        }
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
}

impl ICommittingHandler for TokensTracker {
    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
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
                tracker.on_persist(system, block, snapshot, application_executed_list)
            }) {
                break;
            }
        }
    }
}

impl ICommittedHandler for TokensTracker {
    fn blockchain_committed_handler(&self, system: &dyn Any, _block: &Block) {
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
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
            if !self.run_tracker_action(&track_name, "commit", || tracker.commit()) {
                break;
            }
        }
    }
}

fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic".to_string()
    }
}
