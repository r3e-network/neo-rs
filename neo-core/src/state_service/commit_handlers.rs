//! Blockchain committing/committed handlers for the state service.
//!
//! These handlers wire state root calculation into the block persistence pipeline,
//! mirroring the C# StateService plugin behaviour:
//! - On `Committing`: apply the block's storage change set to the MPT and stage the new root
//! - On `Committed`: persist the staged trie changes and advance the current local root index

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
}

impl StateServiceCommitHandlers {
    /// Creates a new handler with the given state store.
    pub fn new(state_store: Arc<StateStore>) -> Self {
        let exception_policy = state_store.exception_policy();
        Self {
            state_store,
            exception_policy,
            disabled: AtomicBool::new(false),
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
}

impl ICommittingHandler for StateServiceCommitHandlers {
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
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let height = block.index();
            let tracked = snapshot.tracked_items();
            let changes = tracked
                .into_iter()
                .map(|(key, trackable)| (key, trackable.item, trackable.state));
            self.state_store
                .update_local_state_root_snapshot(height, changes);
        }));
        if let Err(payload) = result {
            self.handle_panic(payload, "committing");
        }
    }
}

impl ICommittedHandler for StateServiceCommitHandlers {
    fn blockchain_committed_handler(&self, _system: &dyn Any, block: &Block) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            self.state_store.update_local_state_root(block.index());
        }));
        if let Err(payload) = result {
            self.handle_panic(payload, "committed");
        }
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
