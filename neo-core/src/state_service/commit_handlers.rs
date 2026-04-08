//! Blockchain committing/committed handlers for the state service.
//!
//! These handlers wire state root calculation into the block persistence pipeline,
//! mirroring the C# StateService plugin behaviour:
//! - On `Committing`: apply the block's storage change set to the MPT and stage the new root
//! - On `Committed`: persist the staged trie changes and advance the current local root index

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::i_event_handlers::{ICommittedHandler, ICommittingHandler};
use crate::ledger::{block::Block, blockchain_application_executed::ApplicationExecuted};
use crate::persistence::data_cache::DataCache;
use crate::state_service::StateStore;
use crate::unhandled_exception_policy::UnhandledExceptionPolicy;
use tracing::error;

/// Handlers for wiring state root calculation into block persistence.
pub struct StateServiceCommitHandlers {
    state_store: Arc<StateStore>,
    exception_policy: UnhandledExceptionPolicy,
    disabled: AtomicBool,
    /// Handle to the background thread computing the previous block's MPT state root.
    pending_task: parking_lot::Mutex<Option<JoinHandle<()>>>,
}

impl StateServiceCommitHandlers {
    /// Creates a new handler with the given state store.
    pub fn new(state_store: Arc<StateStore>) -> Self {
        let exception_policy = state_store.exception_policy();
        Self {
            state_store,
            exception_policy,
            disabled: AtomicBool::new(false),
            pending_task: parking_lot::Mutex::new(None),
        }
    }

    fn handle_panic(&self, payload: Box<dyn Any + Send>, phase: &'static str) {
        error!(
            target: "neo::state_service",
            phase,
            error = panic_message(&payload),
            "state service handler panicked"
        );
        match self.exception_policy {
            UnhandledExceptionPolicy::StopPlugin => {
                self.disabled.store(true, Ordering::SeqCst);
            }
            UnhandledExceptionPolicy::StopNode => std::process::exit(1),
            UnhandledExceptionPolicy::Terminate => std::process::abort(),
            UnhandledExceptionPolicy::Ignore | UnhandledExceptionPolicy::Continue => {}
        }
    }

    /// Blocks until any pending background MPT computation completes.
    pub fn flush(&self) {
        if let Some(handle) = self.pending_task.lock().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for StateServiceCommitHandlers {
    fn drop(&mut self) {
        if let Some(handle) = self.pending_task.lock().take() {
            let _ = handle.join();
        }
    }
}

impl ICommittingHandler for StateServiceCommitHandlers {
    fn run_during_fast_sync(&self) -> bool {
        true
    }

    fn blockchain_committing_handler(
        &self,
        _system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }

        // Wait for previous block's MPT to finish before starting this one.
        if let Some(handle) = self.pending_task.lock().take() {
            if handle.join().is_err() {
                self.handle_panic(
                    Box::new("background MPT thread panicked".to_string()),
                    "committing (join)",
                );
                return;
            }
        }

        // Collect tracked items NOW while snapshot is still alive.
        let height = block.index();
        let changes: Vec<_> = snapshot
            .tracked_items()
            .into_iter()
            .map(|(key, trackable)| (key, trackable.item, trackable.state))
            .collect();

        // Spawn background MPT computation.
        let state_store = Arc::clone(&self.state_store);
        let disabled = self.disabled.load(Ordering::Relaxed);
        if disabled {
            return;
        }

        let handle = std::thread::spawn(move || {
            state_store.update_local_state_root_snapshot(height, changes.into_iter());
            state_store.update_local_state_root(height);
        });

        *self.pending_task.lock() = Some(handle);
    }
}

impl ICommittedHandler for StateServiceCommitHandlers {
    fn blockchain_committed_handler(&self, _system: &dyn Any, _block: &Block) {
        // MPT persist is now handled by the background thread spawned in
        // blockchain_committing_handler. No work needed here.
    }
}

fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}
