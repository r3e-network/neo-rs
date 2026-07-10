//! TokensTracker runtime.
//!
//! Provides balance/transfer indexing for NEP-17/NEP-11 standards. This struct is
//! registered as a committing/committed handler to process block events.

use super::settings::TokensTrackerSettings;
use super::trackers::nep_11::Nep11Tracker;
use super::trackers::nep_17::Nep17Tracker;
use super::trackers::tracker_base::Tracker;
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::StandardNativeProvider;
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_payloads::{CommittedHandler, CommittingHandler};
use neo_primitives::panic_message;
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::persistence::{CacheRead, DataCache, Store};
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
pub struct TokensTracker<P = StandardNativeProvider, S: Store = MemoryStore>
where
    P: NativeContractProvider,
{
    settings: TokensTrackerSettings,
    trackers: RwLock<Vec<TrackerRuntime<P, S>>>,
    disabled: AtomicBool,
    _provider: std::marker::PhantomData<P>,
    _store: std::marker::PhantomData<fn(&S)>,
}

enum TrackerRuntime<P, S>
where
    P: NativeContractProvider,
    S: Store,
{
    Nep17(Nep17Tracker<P, S>),
    Nep11(Nep11Tracker<P, S>),
}

impl<P, S> TrackerRuntime<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn track_name(&self) -> &str {
        match self {
            Self::Nep17(tracker) => tracker.track_name(),
            Self::Nep11(tracker) => tracker.track_name(),
        }
    }

    fn reset_batch(&mut self) {
        match self {
            Self::Nep17(tracker) => tracker.reset_batch(),
            Self::Nep11(tracker) => tracker.reset_batch(),
        }
    }

    fn on_persist<B: CacheRead>(
        &mut self,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    ) {
        match self {
            Self::Nep17(tracker) => tracker.on_persist(block, snapshot, application_executed_list),
            Self::Nep11(tracker) => tracker.on_persist(block, snapshot, application_executed_list),
        }
    }

    fn commit(&mut self) -> neo_error::CoreResult<()> {
        match self {
            Self::Nep17(tracker) => tracker.commit(),
            Self::Nep11(tracker) => tracker.commit(),
        }
    }
}

impl<P, S> TokensTracker<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    /// Creates a new TokensTracker with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `settings` - Tracker configuration
    /// * `db` - Database store for balance/transfer data
    /// * `protocol_settings` - Protocol settings (for VM execution)
    pub fn new(
        settings: TokensTrackerSettings,
        db: Arc<S>,
        protocol_settings: Arc<ProtocolSettings>,
        native_contract_provider: Arc<P>,
    ) -> Self {
        let mut trackers = Vec::new();

        if settings.enabled_nep17() {
            trackers.push(TrackerRuntime::Nep17(Nep17Tracker::new(
                Arc::clone(&db),
                settings.max_results,
                settings.track_history,
                Arc::clone(&protocol_settings),
                Arc::clone(&native_contract_provider),
            )));
        }

        if settings.enabled_nep11() {
            trackers.push(TrackerRuntime::Nep11(Nep11Tracker::new(
                Arc::clone(&db),
                settings.max_results,
                settings.track_history,
                Arc::clone(&protocol_settings),
                Arc::clone(&native_contract_provider),
            )));
        }

        Self {
            settings,
            trackers: RwLock::new(trackers),
            disabled: AtomicBool::new(false),
            _provider: std::marker::PhantomData,
            _store: std::marker::PhantomData,
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

impl<P, S> CommittingHandler for TokensTracker<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn blockchain_committing_handler<B: neo_storage::CacheRead>(
        &self,
        network: u32,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    ) {
        if network != self.settings.network {
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

impl<P, S> CommittedHandler for TokensTracker<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn blockchain_committed_handler(&self, network: u32, _block: &Block) {
        if network != self.settings.network {
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
