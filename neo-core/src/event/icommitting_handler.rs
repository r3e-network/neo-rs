
use neo::prelude::*;
use neo::ledger::Blockchain;
use neo::network::p2p::payloads::Block;
use neo::persistence::DataCache;

/// This module contains the ICommittingHandler trait.
pub mod event_handlers {
    use crate::block::Block;
    use crate::neo_system::NeoSystem;
    use crate::persistence::DataCache;
    use super::*;

    /// Trait for handling the Committing event from the Blockchain.
    pub trait ICommittingHandler {
        /// Handler for the Blockchain Committing event.
        ///
        /// This function is triggered when a new block is committing, and the state is still in the cache.
        ///
        /// # Arguments
        ///
        /// * `system` - A reference to the NeoSystem object.
        /// * `block` - The block that is being committed.
        /// * `snapshot` - The current data snapshot.
        /// * `application_executed_list` - A list of executed applications associated with the block.
        fn blockchain_committing_handler(
            &self,
            system: &NeoSystem,
            block: &Block,
            snapshot: &DataCache,
            application_executed_list: &[Blockchain::ApplicationExecuted],
        );
    }
}
