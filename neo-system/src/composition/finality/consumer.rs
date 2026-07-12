//! Finalized-block consumer capability.

use neo_blockchain::FinalizedBlock;
use neo_storage::CacheRead;

/// Statically dispatched consumer for durably committed block outcomes.
///
/// Implementations own non-consensus projections only. Consensus-critical
/// StateService, index, and archive durability remains in
/// [`crate::BlockCommitHooks::block_committing`] and its durability fence.
pub trait FinalizedBlockConsumer<B>: Send + Sync + 'static
where
    B: CacheRead,
{
    /// Applies one finalized notification in canonical height order.
    fn consume(&self, finalized: &FinalizedBlock<B>) -> Result<(), String>;
}
