use std::fmt;
use std::sync::Arc;

use crate::ledger_provider::{BlockProvider, StorageLedgerProvider};
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::Block;
use neo_primitives::{UInt160, UInt256};
use neo_storage::DataCache;

/// Parent-header data needed to verify a child block's consensus witness.
#[derive(Debug, Clone, Copy)]
pub struct ParentHeaderContext {
    /// Hash of the parent block.
    pub hash: UInt256,
    /// Height of the parent block.
    pub index: u32,
    /// Parent timestamp in milliseconds.
    pub timestamp: u64,
    /// Parent `NextConsensus`, the account that must authorize this header.
    pub next_consensus: UInt160,
}

/// Narrow context required by [`super::NeoConsensusWitnessStage`].
pub trait ConsensusWitnessContext: Send + Sync + fmt::Debug + 'static {
    /// Returns the protocol settings used by witness verification.
    fn settings(&self) -> Arc<ProtocolSettings>;

    /// Returns the snapshot used for contract lookups during verification.
    fn snapshot(&self) -> &DataCache;

    /// Returns the explicit native provider used by NeoVM host calls.
    fn native_contract_provider(&self) -> Option<Arc<dyn NativeContractProvider>>;

    /// Resolves the previous header context for `block`.
    fn parent_header(&self, block: &Block) -> CoreResult<ParentHeaderContext>;
}

/// Snapshot-backed consensus-witness context used by service handlers.
#[derive(Clone)]
pub struct SnapshotConsensusWitnessContext {
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<DataCache>,
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl fmt::Debug for SnapshotConsensusWitnessContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotConsensusWitnessContext")
            .field("validators_count", &self.settings.validators_count)
            .field("native_contract_provider", &"NativeContractProvider")
            .finish_non_exhaustive()
    }
}

impl SnapshotConsensusWitnessContext {
    /// Creates a context over an immutable store snapshot and explicit native
    /// provider.
    #[must_use]
    pub fn new(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache>,
        native_contract_provider: Arc<dyn NativeContractProvider>,
    ) -> Self {
        Self {
            settings,
            snapshot,
            native_contract_provider,
        }
    }
}

impl ConsensusWitnessContext for SnapshotConsensusWitnessContext {
    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn snapshot(&self) -> &DataCache {
        self.snapshot.as_ref()
    }

    fn native_contract_provider(&self) -> Option<Arc<dyn NativeContractProvider>> {
        Some(Arc::clone(&self.native_contract_provider))
    }

    fn parent_header(&self, block: &Block) -> CoreResult<ParentHeaderContext> {
        let prev = StorageLedgerProvider::new(self.snapshot.as_ref())
            .header_by_hash(block.header.prev_hash())?
            .ok_or_else(|| CoreError::other("previous block not found"))?;

        Ok(ParentHeaderContext {
            hash: prev.hash(),
            index: prev.index(),
            timestamp: prev.timestamp(),
            next_consensus: *prev.next_consensus(),
        })
    }
}
