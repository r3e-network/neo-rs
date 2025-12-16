//! Blockchain committing/committed handlers for the state service.
//!
//! These handlers wire state root calculation into the block persistence pipeline,
//! mirroring the C# StateService plugin behaviour:
//! - On `Committing`: apply the block's storage change set to the MPT and stage the new root
//! - On `Committed`: persist the staged trie changes and advance the current local root index

use std::any::Any;
use std::sync::Arc;

use crate::i_event_handlers::{ICommittedHandler, ICommittingHandler};
use crate::ledger::{block::Block, blockchain_application_executed::ApplicationExecuted};
use crate::persistence::data_cache::DataCache;
use crate::state_service::StateStore;

/// Handlers for wiring state root calculation into block persistence.
/// Currently unused but reserved for neo-node integration.
#[derive(Clone)]
#[allow(dead_code)]
pub struct StateServiceCommitHandlers {
    state_store: Arc<StateStore>,
}

impl StateServiceCommitHandlers {
    /// Creates a new handler with the given state store.
    #[allow(dead_code)]
    pub fn new(state_store: Arc<StateStore>) -> Self {
        Self { state_store }
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
        let height = block.index();
        let tracked = snapshot.tracked_items();
        let changes = tracked
            .into_iter()
            .map(|(key, trackable)| (key, trackable.item, trackable.state));
        self.state_store
            .update_local_state_root_snapshot(height, changes);
    }
}

impl ICommittedHandler for StateServiceCommitHandlers {
    fn blockchain_committed_handler(&self, _system: &dyn Any, block: &Block) {
        self.state_store.update_local_state_root(block.index());
    }
}
