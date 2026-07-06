use std::fmt;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_native_contracts::LedgerContract;
use neo_primitives::UInt256;
use neo_storage::DataCache;

/// Context trait providing the stateful dependencies needed for full validation.
///
/// This trait is intentionally narrow — it exposes only what the validate stage
/// needs, not the full `SystemContext`. This makes it easy to mock in tests.
pub trait ValidateContext: Send + Sync + fmt::Debug + 'static {
    /// Returns the protocol settings (validator count, genesis timestamp, etc.).
    fn settings(&self) -> Arc<ProtocolSettings>;

    /// Returns the previous block hash at the given height, or `None` if the
    /// height is not yet in the store.
    ///
    /// The stage uses this to verify header chaining (prev_hash + height).
    fn prev_block_hash(&self, height: u32) -> Option<UInt256>;

    /// Returns the previous block timestamp, or `None` if not available.
    ///
    /// The stage uses this to verify timestamp progression.
    fn prev_block_timestamp(&self, height: u32) -> Option<u64>;

    /// Returns the validators count for primary index validation.
    fn validators_count(&self) -> i32;
}

/// Snapshot-backed validate context used by service handlers.
#[derive(Clone)]
pub struct SnapshotValidateContext {
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<DataCache>,
}

impl fmt::Debug for SnapshotValidateContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotValidateContext")
            .field("validators_count", &self.settings.validators_count)
            .finish_non_exhaustive()
    }
}

impl SnapshotValidateContext {
    /// Creates a validate context over an immutable store snapshot.
    #[must_use]
    pub fn new(settings: Arc<ProtocolSettings>, snapshot: Arc<DataCache>) -> Self {
        Self { settings, snapshot }
    }

    fn ledger(&self) -> LedgerContract {
        LedgerContract::new()
    }
}

impl ValidateContext for SnapshotValidateContext {
    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn prev_block_hash(&self, height: u32) -> Option<UInt256> {
        self.ledger()
            .get_block_hash(self.snapshot.as_ref(), height)
            .ok()
            .flatten()
    }

    fn prev_block_timestamp(&self, height: u32) -> Option<u64> {
        let ledger = self.ledger();
        let hash = ledger
            .get_block_hash(self.snapshot.as_ref(), height)
            .ok()
            .flatten()?;
        ledger
            .get_trimmed_block(self.snapshot.as_ref(), &hash)
            .ok()
            .flatten()
            .map(|block| block.header.timestamp())
    }

    fn validators_count(&self) -> i32 {
        self.settings.validators_count
    }
}
