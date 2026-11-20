use neo_protocol::execution::{ApplicationExecuted, Block};
use neo_storage::persistence::PersistenceDataCache;
use neo_wallets::Wallet;
use std::{any::Any, sync::Arc};

pub type CoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Handler of Committed event from Blockchain.
/// Triggered after a new block is committed and state has been updated.
pub trait CommittedHandler {
    /// Called after a block has been committed and its state updated.
    fn blockchain_committed_handler(&self, system: &dyn Any, block: &Block);
}

/// Handler of Committing event from Blockchain.
/// Triggered when a new block is committing, state is still in the cache.
pub trait CommittingHandler {
    /// Indicates whether this handler should run during fast sync
    fn run_during_fast_sync(&self) -> bool {
        false
    }

    /// Called when a block is committing, while its state is still in the cache.
    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &PersistenceDataCache,
        application_executed_list: &[ApplicationExecuted],
    );

    /// Fallible committing hook used by protocol-critical handlers.
    fn try_blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &PersistenceDataCache,
        application_executed_list: &[ApplicationExecuted],
    ) -> CoreResult<()> {
        self.blockchain_committing_handler(system, block, snapshot, application_executed_list);
        Ok(())
    }

    /// Discards state staged by the committing hook after a failed participant
    /// vote, store commit, or runtime-cache update. Stateless handlers need no
    /// rollback and retain the default no-op.
    fn blockchain_rollback_handler(
        &self,
        _system: &dyn Any,
        _block: &Block,
        _snapshot: &PersistenceDataCache,
    ) -> CoreResult<()> {
        Ok(())
    }
}

/// Handler of WalletChanged event from the WalletProvider.
/// Triggered when a new wallet is assigned to the node.
pub trait WalletChangedHandler {
    /// Called when the node's active wallet changes.
    fn wallet_provider_wallet_changed_handler(
        &self,
        sender: &dyn Any,
        wallet: Option<Arc<dyn Wallet>>,
    );
}
