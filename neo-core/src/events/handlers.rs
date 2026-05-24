use crate::{
    error::CoreResult,
    ledger::{
        block::Block,
        blockchain_application_executed::ApplicationExecuted,
    },
    network::message::Message,
    wallets::Wallet,
};
use std::{any::Any, sync::Arc};

/// Handler of Committed event from Blockchain.
/// Triggered after a new block is committed and state has been updated.
pub trait ICommittedHandler {
    fn blockchain_committed_handler(&self, system: &dyn Any, block: &Block);
}

/// Handler of Committing event from Blockchain.
/// Triggered when a new block is committing, state is still in the cache.
pub trait ICommittingHandler {
    /// Indicates whether this handler should run during fast sync
    fn run_during_fast_sync(&self) -> bool {
        false
    }

    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &crate::persistence::data_cache::DataCache,
        application_executed_list: &[ApplicationExecuted],
    );

    /// Fallible committing hook used by protocol-critical handlers.
    fn try_blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &crate::persistence::data_cache::DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) -> CoreResult<()> {
        self.blockchain_committing_handler(system, block, snapshot, application_executed_list);
        Ok(())
    }
}

/// Handler of MessageReceived event from RemoteNode.
/// Triggered when a new message is received from a peer.
pub trait IMessageReceivedHandler {
    fn remote_node_message_received_handler(&self, system: &dyn Any, message: &Message) -> bool;
}

/// Handler of WalletChanged event from the IWalletProvider.
/// Triggered when a new wallet is assigned to the node.
pub trait IWalletChangedHandler {
    fn wallet_provider_wallet_changed_handler(
        &self,
        sender: &dyn Any,
        wallet: Option<Arc<dyn Wallet>>,
    );
}
