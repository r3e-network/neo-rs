use std::borrow::Borrow;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::Block;
use neo_storage::{CacheRead, DataCache};

use crate::native_persist::{NativePersistOptions, NativePersistResources};
use crate::service_context::BlockPersistContext;

use super::planner::{EmptyBlockFastForwardRequest, plan_empty_block_fast_forward};
use super::provider::{EmptyBlockFastForwardNativeProvider, NativeEmptyBlockFastForwardProvider};
use super::types::EmptyBlockFastForwardPlan;

/// Empty-block fast-forward writes staged in an isolated child cache.
pub struct StagedEmptyBlockFastForward<C: CacheRead> {
    /// Staged writes, isolated from the canonical snapshot until commit.
    snapshot: Arc<DataCache<C>>,
    /// Eligible interval covered by this staged write.
    pub plan: EmptyBlockFastForwardPlan,
}

impl<C: CacheRead> StagedEmptyBlockFastForward<C> {
    /// Returns the staged snapshot for tests and committing gates.
    pub fn snapshot(&self) -> &DataCache<C> {
        self.snapshot.as_ref()
    }

    /// Publishes the staged writes into the canonical parent snapshot.
    pub fn commit(&self) {
        self.snapshot.commit();
    }
}

/// Stages a state-equivalent fast-forward for a contiguous empty-block run.
///
/// Ledger history is written for every block, the current-block pointer
/// advances to the interval end, and NEO/GAS empty-block effects are aggregated
/// through `neo-native-contracts` storage helpers.
pub fn stage_empty_block_fast_forward<T, P, C>(
    snapshot: Arc<DataCache<C>>,
    blocks: &[T],
    settings: &ProtocolSettings,
    persist_options: NativePersistOptions,
    persist_context: BlockPersistContext,
    resources: &NativePersistResources<P>,
    current_height: u32,
) -> CoreResult<StagedEmptyBlockFastForward<C>>
where
    T: Borrow<Block>,
    P: NativeContractProvider + 'static,
    C: CacheRead,
{
    let plan = plan_empty_block_fast_forward(EmptyBlockFastForwardRequest {
        current_height,
        blocks,
        settings,
        resources,
        persist_options,
        persist_context,
    })
    .map_err(|error| CoreError::invalid_operation(error.to_string()))?;

    let committee_count = settings.committee_members_count();
    if committee_count == 0 {
        return Err(CoreError::invalid_operation(
            "empty-block fast-forward requires a non-empty committee",
        ));
    }
    let block_cache = Arc::new(snapshot.clone_cache());
    let mut last_persisted = None;
    for block in blocks {
        let block = block.borrow();
        let block_hash = block
            .try_hash()
            .map_err(|e| CoreError::invalid_operation(format!("empty fast-forward hash: {e}")))?;
        crate::ledger_records::LedgerRecords::write_on_persist_records(
            &block_cache,
            block,
            &block_hash,
        )?;
        last_persisted = Some((block_hash, block.index()));
    }

    let (last_hash, last_index) = last_persisted
        .ok_or_else(|| CoreError::invalid_operation("empty fast-forward candidate is empty"))?;
    crate::ledger_records::LedgerRecords::write_post_persist_record(
        &block_cache,
        &last_hash,
        last_index,
    )?;
    let empty_block_native_provider =
        NativeEmptyBlockFastForwardProvider::new(resources.provider());
    empty_block_native_provider.fast_forward_empty_block_rewards(
        &block_cache,
        settings,
        plan.start,
        plan.end,
    )?;

    Ok(StagedEmptyBlockFastForward {
        snapshot: block_cache,
        plan,
    })
}
