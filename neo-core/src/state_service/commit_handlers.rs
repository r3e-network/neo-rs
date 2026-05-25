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

use crate::error::{CoreError, CoreResult};
use crate::i_event_handlers::{CommittedHandler, CommittingHandler};
use crate::ledger::{block::Block, blockchain_application_executed::ApplicationExecuted};
use crate::persistence::data_cache::DataCache;
use crate::state_service::StateStore;
use crate::unhandled_exception_policy::{panic_message, UnhandledExceptionPolicy};
use tracing::error;

/// Handlers for wiring state root calculation into block persistence.
pub struct StateServiceCommitHandlers {
    state_store: Arc<StateStore>,
    exception_policy: UnhandledExceptionPolicy,
    disabled: AtomicBool,
    /// Handle to the background thread computing the previous block's MPT state root.
    pending_task: parking_lot::Mutex<Option<JoinHandle<Result<(), String>>>>,
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
            error = panic_message(payload.as_ref(), "unknown panic payload"),
            "state service handler panicked"
        );
        self.apply_exception_policy();
    }

    fn handle_error(&self, err: &CoreError, phase: &'static str) {
        error!(
            target: "neo::state_service",
            phase,
            error = %err,
            "state service handler failed"
        );
        self.apply_exception_policy();
    }

    fn apply_exception_policy(&self) {
        self.exception_policy
            .apply(|| self.disabled.store(true, Ordering::SeqCst));
    }

    fn join_pending(&self, phase: &'static str) -> CoreResult<()> {
        let Some(handle) = self.pending_task.lock().take() else {
            return Ok(());
        };

        match handle.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(CoreError::system(format!(
                "state service commit handler failed during {phase}: {err}"
            ))),
            Err(payload) => {
                let message = panic_message(payload.as_ref(), "unknown panic payload");
                self.handle_panic(payload, phase);
                Err(CoreError::system(format!(
                    "state service commit handler panicked during {phase}: {message}"
                )))
            }
        }
    }

    /// Blocks until any pending background MPT computation completes.
    #[allow(dead_code)]
    pub fn flush(&self) -> CoreResult<()> {
        self.join_pending("flush")
    }
}

impl Drop for StateServiceCommitHandlers {
    fn drop(&mut self) {
        if let Some(handle) = self.pending_task.lock().take() {
            let _ = handle.join();
        }
    }
}

impl CommittingHandler for StateServiceCommitHandlers {
    fn run_during_fast_sync(&self) -> bool {
        true
    }

    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        if let Err(err) = self.try_blockchain_committing_handler(
            system,
            block,
            snapshot,
            application_executed_list,
        ) {
            self.handle_error(&err, "committing");
        }
    }

    fn try_blockchain_committing_handler(
        &self,
        _system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) -> CoreResult<()> {
        if self.disabled.load(Ordering::Relaxed) {
            return Err(CoreError::system(
                "state service committing handler is disabled after a previous failure",
            ));
        }

        // Wait for previous block's MPT to finish before starting this one.
        self.join_pending("committing previous block")?;

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
            return Err(CoreError::system(
                "state service committing handler is disabled after a previous failure",
            ));
        }

        let handle = std::thread::spawn(move || {
            state_store.update_local_state_root_snapshot(height, changes.into_iter())?;
            state_store.update_local_state_root(height)
        });

        *self.pending_task.lock() = Some(handle);
        self.join_pending("committing current block")
    }
}

impl CommittedHandler for StateServiceCommitHandlers {
    fn blockchain_committed_handler(&self, _system: &dyn Any, _block: &Block) {
        // MPT persist is now handled by the background thread spawned in
        // blockchain_committing_handler. No work needed here.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::block_header::BlockHeader;
    use crate::smart_contract::{StorageItem, StorageKey};
    use crate::state_service::state_store::{StateServiceSettings, StateStoreBackend};
    use crate::{UInt160, UInt256, Witness};

    struct FailingStateStoreBackend;

    impl StateStoreBackend for FailingStateStoreBackend {
        fn try_get(&self, _key: &[u8]) -> Option<Vec<u8>> {
            None
        }

        fn put(&self, _key: Vec<u8>, _value: Vec<u8>) {}

        fn delete(&self, _key: &[u8]) {}

        fn commit(&self) -> Result<(), String> {
            Err("injected state root commit failure".to_string())
        }
    }

    fn state_store_with_policy(exception_policy: UnhandledExceptionPolicy) -> Arc<StateStore> {
        let settings = StateServiceSettings {
            exception_policy,
            ..StateServiceSettings::default()
        };
        Arc::new(StateStore::new(
            Arc::new(FailingStateStoreBackend),
            settings,
        ))
    }

    fn test_block(index: u32) -> Block {
        Block::new(
            BlockHeader::new(
                0,
                UInt256::zero(),
                UInt256::zero(),
                index as u64,
                index as u64,
                index,
                0,
                UInt160::zero(),
                vec![Witness::new()],
            ),
            Vec::new(),
        )
    }

    #[test]
    fn try_committing_handler_returns_err_when_current_state_root_commit_fails() {
        let handler = StateServiceCommitHandlers::new(state_store_with_policy(
            UnhandledExceptionPolicy::StopPlugin,
        ));
        let snapshot = DataCache::new(false);

        let err = handler
            .try_blockchain_committing_handler(&(), &test_block(1), &snapshot, &[])
            .expect_err("state root commit failure should stop block commit");

        assert!(err
            .to_string()
            .contains("injected state root commit failure"));
    }

    #[test]
    fn committing_handler_disables_after_error_when_policy_stops_plugin() {
        let handler = StateServiceCommitHandlers::new(state_store_with_policy(
            UnhandledExceptionPolicy::StopPlugin,
        ));
        let snapshot = DataCache::new(false);

        handler.blockchain_committing_handler(&(), &test_block(1), &snapshot, &[]);

        let err = handler
            .try_blockchain_committing_handler(&(), &test_block(2), &snapshot, &[])
            .expect_err("stop-plugin policy should disable future state root commits");

        assert!(err
            .to_string()
            .contains("disabled after a previous failure"));
    }

    #[test]
    fn committing_handler_keeps_running_after_error_when_policy_continues() {
        let handler = StateServiceCommitHandlers::new(state_store_with_policy(
            UnhandledExceptionPolicy::Continue,
        ));
        let snapshot = DataCache::new(false);

        handler.blockchain_committing_handler(&(), &test_block(1), &snapshot, &[]);

        let err = handler
            .try_blockchain_committing_handler(&(), &test_block(2), &snapshot, &[])
            .expect_err("continue policy should leave handler enabled");

        assert!(err
            .to_string()
            .contains("injected state root commit failure"));
    }

    #[test]
    fn try_committing_handler_stages_storage_changes_before_commit_gate() {
        let state_store = Arc::new(StateStore::new_in_memory());
        let handler = StateServiceCommitHandlers::new(Arc::clone(&state_store));
        let snapshot = DataCache::new(false);
        snapshot.add(
            StorageKey::new(123, b"state-key".to_vec()),
            StorageItem::from_bytes(b"state-value".to_vec()),
        );

        handler
            .try_blockchain_committing_handler(&(), &test_block(1), &snapshot, &[])
            .expect("state root commit");

        assert_eq!(state_store.local_root_index(), Some(1));
    }
}
