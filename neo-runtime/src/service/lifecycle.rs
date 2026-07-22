//! Node-service lifecycle capability contracts.
//!
//! ## Boundary
//!
//! This module owns callbacks shared by node services that observe canonical
//! block finalization or wallet replacement. Payload crates only define the
//! records carried through these callbacks.

use std::sync::Arc;

use neo_payloads::{ApplicationExecuted, Block};
use neo_storage::{CacheRead, DataCache};

/// Service capability for the post-commit half of a canonical block update.
pub trait CommittedHandler: Send + Sync {
    /// Called after the service's pending block projection may be committed.
    fn blockchain_committed_handler(&self, network: u32, block: &Block);
}

/// Service capability for deriving a projection from canonical block state.
pub trait CommittingHandler: Send + Sync {
    /// Called with the canonical snapshot and execution records for one block.
    fn blockchain_committing_handler<B: CacheRead>(
        &self,
        network: u32,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    );
}

/// Service capability for a projection derived after Ledger durability.
///
/// Implementors receive a snapshot that cannot be mutated by a later
/// observer-visible block until this call returns.
pub trait FinalizedHandler: Send + Sync {
    /// Derives and commits one projection from a durably finalized block.
    fn blockchain_finalized_handler<B: CacheRead>(
        &self,
        network: u32,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    );
}

impl<T> FinalizedHandler for T
where
    T: CommittingHandler + CommittedHandler,
{
    fn blockchain_finalized_handler<B: CacheRead>(
        &self,
        network: u32,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    ) {
        self.blockchain_committing_handler(network, block, snapshot, application_executed_list);
        self.blockchain_committed_handler(network, block);
    }
}

/// Service capability for reacting to an active-wallet replacement.
pub trait WalletChangedHandler: Send + Sync {
    /// Concrete event sender type selected by the dispatcher.
    type Sender: ?Sized;

    /// Concrete wallet handle selected by the dispatcher.
    type Wallet: Send + Sync + 'static;

    /// Called when the active wallet changes.
    fn wallet_provider_wallet_changed_handler(
        &self,
        sender: &Self::Sender,
        wallet: Option<Arc<Self::Wallet>>,
    );
}

#[cfg(test)]
#[path = "../tests/service/lifecycle.rs"]
mod tests;
