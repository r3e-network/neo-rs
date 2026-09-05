//! Blockchain committing/committed handlers for the state service.
//!
//! These handlers wire state root calculation into the block persistence pipeline,
//! mirroring the C# StateService plugin behaviour:
//! - On `Committing`: apply the block's storage change set to the MPT and stage the new root
//! - On `Committed`: persist the staged trie changes and advance the current local root index

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{CoreError, CoreResult};
use crate::i_event_handlers::{CommittedHandler, CommittingHandler};
use crate::ledger::{block::Block, blockchain_application_executed::ApplicationExecuted};
use crate::persistence::data_cache::DataCache;
use crate::state_service::StateStore;
use crate::unhandled_exception_policy::{UnhandledExceptionPolicy, panic_message};
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

        // Collect tracked items NOW while snapshot is still alive.
        let height = block.index();
        let changes: Vec<_> = snapshot
            .tracked_items()
            .into_iter()
            .map(|(key, trackable)| (key, trackable.item, trackable.state))
            .collect();

        // Compute and stage the root synchronously. The previous
        // implementation spawned a thread and immediately joined it, paying
        // a thread-creation cost per block without overlapping any work
        // (review §5.1). Panics are contained exactly as the old join() path
        // contained them, and the staging/error gate is unchanged: only
        // compute and stage the root here — persisting it before the block
        // transaction commits would advance the state root on failed blocks.
        let state_store = Arc::clone(&self.state_store);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            state_store.update_local_state_root_snapshot(height, changes.into_iter())
        }));

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(CoreError::system(format!(
                "state service commit handler failed during committing: {err}"
            ))),
            Err(payload) => {
                let message = panic_message(payload.as_ref(), "unknown panic payload");
                self.handle_panic(payload, "committing");
                Err(CoreError::system(format!(
                    "state service commit handler panicked during committing: {message}"
                )))
            }
        }
    }
}

impl CommittedHandler for StateServiceCommitHandlers {
    fn blockchain_committed_handler(&self, _system: &dyn Any, block: &Block) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }

        let height = block.index();
        if let Err(err) = self.state_store.update_local_state_root(height) {
            let error = CoreError::system(format!(
                "state service commit failed for block {height}: {err}"
            ));
            self.handle_error(&error, "committed");
        }
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
    fn committing_stages_root_without_advancing_until_block_is_committed() {
        let state_store = state_store_with_policy(UnhandledExceptionPolicy::StopPlugin);
        let handler = StateServiceCommitHandlers::new(Arc::clone(&state_store));
        let snapshot = DataCache::new(false);

        handler
            .try_blockchain_committing_handler(&(), &test_block(1), &snapshot, &[])
            .expect("staging must not commit the state backend");
        assert_eq!(state_store.local_root_index(), None);

        handler.blockchain_committed_handler(&(), &test_block(1));
        assert!(handler.disabled.load(Ordering::Relaxed));
        assert_eq!(state_store.local_root_index(), None);
    }

    #[test]
    fn committing_handler_disables_after_error_when_policy_stops_plugin() {
        let handler = StateServiceCommitHandlers::new(state_store_with_policy(
            UnhandledExceptionPolicy::StopPlugin,
        ));
        let snapshot = DataCache::new(false);

        handler
            .try_blockchain_committing_handler(&(), &test_block(1), &snapshot, &[])
            .expect("state root should be staged");
        handler.blockchain_committed_handler(&(), &test_block(1));

        let err = handler
            .try_blockchain_committing_handler(&(), &test_block(2), &snapshot, &[])
            .expect_err("stop-plugin policy should disable future state root commits");

        assert!(
            err.to_string()
                .contains("disabled after a previous failure")
        );
    }

    #[test]
    fn committing_handler_keeps_running_after_error_when_policy_continues() {
        let handler = StateServiceCommitHandlers::new(state_store_with_policy(
            UnhandledExceptionPolicy::Continue,
        ));
        let snapshot = DataCache::new(false);

        handler
            .try_blockchain_committing_handler(&(), &test_block(1), &snapshot, &[])
            .expect("state root should be staged");
        handler.blockchain_committed_handler(&(), &test_block(1));

        handler
            .try_blockchain_committing_handler(&(), &test_block(2), &snapshot, &[])
            .expect("continue policy should keep staging available");
        handler.blockchain_committed_handler(&(), &test_block(2));
        assert!(!handler.disabled.load(Ordering::Relaxed));

        handler
            .try_blockchain_committing_handler(&(), &test_block(3), &snapshot, &[])
            .expect("continue policy should allow subsequent staging");
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
            .expect("state root should be staged");

        assert_eq!(state_store.local_root_index(), None);
        handler.blockchain_committed_handler(&(), &test_block(1));
        assert_eq!(state_store.local_root_index(), Some(1));
    }
}
