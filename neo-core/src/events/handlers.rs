use crate::{
    error::CoreResult,
    ledger::{
        block::Block,
        blockchain_application_executed::ApplicationExecuted,
        transaction_removed_event_args::TransactionRemovedEventArgs,
    },
    network::{message::Message, p2p::payloads::Transaction},
    smart_contract::{
        application_engine::ApplicationEngine, log_event_args::LogEventArgs,
        notify_event_args::NotifyEventArgs,
    },
    wallets::Wallet,
};
use crate::extensions::log_level::LogLevel;
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

/// Handler of Logging event from Utility.
/// Triggered when a new log is added by calling Utility.Log.
pub trait ILoggingHandler {
    fn utility_logging_handler(&self, source: &str, level: LogLevel, message: &str);
}

/// Handler of Log event from the ApplicationEngine.
/// Triggered when a contract calls System.Runtime.Log.
pub trait ILogHandler {
    fn application_engine_log_handler(
        &self,
        sender: &ApplicationEngine,
        log_event_args: &LogEventArgs,
    );
}

/// Handler of MessageReceived event from RemoteNode.
/// Triggered when a new message is received from a peer.
pub trait IMessageReceivedHandler {
    fn remote_node_message_received_handler(&self, system: &dyn Any, message: &Message) -> bool;
}

/// Handler of Notify event from ApplicationEngine.
/// Triggered when a contract calls System.Runtime.Notify.
pub trait INotifyHandler {
    fn application_engine_notify_handler(
        &self,
        sender: &ApplicationEngine,
        notify_event_args: &NotifyEventArgs,
    );
}

/// Handler of ServiceAdded event from the NeoSystem.
/// Triggered when a service is added to the NeoSystem.
pub trait IServiceAddedHandler {
    fn neo_system_service_added_handler(
        &self,
        sender: &dyn Any,
        service: &dyn Any,
    );
}

/// Handler of TransactionAdded event from the MemoryPool.
/// Triggered when a transaction is added to the MemoryPool.
pub trait ITransactionAddedHandler {
    fn memory_pool_transaction_added_handler(&self, sender: &dyn Any, tx: &Transaction);
}

/// Handler of TransactionRemoved event from MemoryPool.
/// Triggered when a transaction is removed from the MemoryPool.
pub trait ITransactionRemovedHandler {
    fn memory_pool_transaction_removed_handler(
        &self,
        sender: &dyn Any,
        tx: &TransactionRemovedEventArgs,
    );
}

/// Handler of WalletChanged event from the IWalletProvider.
/// Triggered when a new wallet is assigned to the node.
pub trait IWalletChangedHandler {
    fn i_wallet_provider_wallet_changed_handler(
        &self,
        sender: &dyn Any,
        wallet: Option<Arc<dyn Wallet>>,
    );
}
