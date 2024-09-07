

/// This module contains the ICommittedHandler trait.
pub mod event_handlers {
    use crate::block::Block;
    use crate::neo_system::NeoSystem;
    use super::*;

    /// Trait for handling the Committed event from the Blockchain.
    pub trait ICommittedHandler {
        /// Handler for the Blockchain Committed event.
        ///
        /// This function is triggered after a new block is committed and the state has been updated.
        ///
        /// # Arguments
        ///
        /// * `system` - A reference to the NeoSystem object.
        /// * `block` - The committed Block.
        fn blockchain_committed_handler(&self, system: &NeoSystem, block: &Block);
    }
}
