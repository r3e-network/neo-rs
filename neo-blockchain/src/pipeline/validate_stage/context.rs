use std::fmt;
use std::sync::Arc;

use crate::ledger_provider::{BlockProvider, StorageLedgerProvider};
use neo_config::{ChainSpecProvider, NeoChainSpec};
use neo_primitives::UInt256;
use neo_storage::{CacheRead, DataCache};

/// Context trait providing the stateful dependencies needed for full validation.
///
/// This trait is intentionally narrow — it exposes only what the validate stage
/// needs, not the full `SystemContext`. This makes it easy to mock in tests.
pub trait ValidateContext:
    ChainSpecProvider<ChainSpec = NeoChainSpec> + Send + Sync + fmt::Debug + 'static
{
    /// Returns the previous block hash at the given height, or `None` if the
    /// height is not yet in the store.
    ///
    /// The stage uses this to verify header chaining (prev_hash + height).
    fn prev_block_hash(&self, height: u32) -> Option<UInt256>;

    /// Returns the previous block timestamp, or `None` if not available.
    ///
    /// The stage uses this to verify timestamp progression.
    fn prev_block_timestamp(&self, height: u32) -> Option<u64>;
}

/// Snapshot-backed validate context used by service handlers.
#[derive(Clone)]
pub struct SnapshotValidateContext<B: CacheRead> {
    chain_spec: Arc<NeoChainSpec>,
    snapshot: Arc<DataCache<B>>,
}

impl<B: CacheRead> fmt::Debug for SnapshotValidateContext<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotValidateContext")
            .field(
                "validators_count",
                &self.chain_spec.protocol_settings().validators_count,
            )
            .finish_non_exhaustive()
    }
}

impl<B: CacheRead> SnapshotValidateContext<B> {
    /// Creates a validate context over an immutable store snapshot.
    #[must_use]
    pub fn new(chain_spec: Arc<NeoChainSpec>, snapshot: Arc<DataCache<B>>) -> Self {
        Self {
            chain_spec,
            snapshot,
        }
    }

    fn provider(&self) -> StorageLedgerProvider<'_, B> {
        StorageLedgerProvider::new(self.snapshot.as_ref())
    }
}

impl<B: CacheRead> ChainSpecProvider for SnapshotValidateContext<B> {
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        Arc::clone(&self.chain_spec)
    }
}

impl<B: CacheRead> ValidateContext for SnapshotValidateContext<B> {
    fn prev_block_hash(&self, height: u32) -> Option<UInt256> {
        self.provider().block_hash_by_index(height).ok().flatten()
    }

    fn prev_block_timestamp(&self, height: u32) -> Option<u64> {
        self.provider()
            .header_by_index(height)
            .ok()
            .flatten()
            .map(|header| header.timestamp())
    }
}
