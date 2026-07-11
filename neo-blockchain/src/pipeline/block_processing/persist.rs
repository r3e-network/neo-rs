use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::Block;
use neo_storage::{CacheRead, DataCache};
use tracing::{debug, error};

use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

pub(crate) struct BatchPersistResources<P, B>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
{
    pub(crate) snapshot: Arc<DataCache<B>>,
    pub(crate) settings: Arc<ProtocolSettings>,
    pub(crate) native_persist: crate::native_persist::NativePersistResources<P>,
}

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Persist a consecutive block sequence: run the C#
    /// `Blockchain.Persist` pipeline (native OnPersist + ledger
    /// records, per-transaction Application execution, native
    /// PostPersist) when the system context exposes a store snapshot.
    /// The pipeline stages all writes in a child cache and commits
    /// them into the snapshot only when the whole sequence succeeds
    /// (see [`crate::native_persist`]). Store-less contexts are reserved for
    /// lightweight tests that exercise the command loop without durable
    /// native-contract persistence.
    pub(crate) async fn persist_block_sequence(&self, block: Arc<Block>) -> bool {
        self.persist_block_sequence_with_options(
            block,
            crate::native_persist::NativePersistOptions::default(),
        )
        .await
    }

    pub(crate) async fn persist_block_sequence_with_options(
        &self,
        block: Arc<Block>,
        options: crate::native_persist::NativePersistOptions,
    ) -> bool {
        let resources = match self.batch_persist_resources(block.index()) {
            Ok(Some(resources)) => resources,
            Ok(None) => return true,
            Err(err) => {
                error!(
                    target: "neo",
                    %err,
                    "block persistence pipeline resource setup failed"
                );
                return false;
            }
        };
        let persist_context = if options.capture_replay_artifacts {
            BlockPersistContext::live()
        } else {
            BlockPersistContext::trusted_replay()
        };
        self.persist_block_sequence_with_resources(block, options, persist_context, &resources)
    }

    pub(crate) fn batch_persist_resources(
        &self,
        index: u32,
    ) -> neo_error::CoreResult<Option<BatchPersistResources<S::NativeProvider, S::CacheBacking>>>
    {
        let Some(snapshot) = self.system.store_snapshot() else {
            debug!(
                target: "neo",
                index,
                "persist_block_sequence: no store snapshot exposed; skipping durable native persistence for store-less context"
            );
            return Ok(None);
        };
        let native_contract_provider = self.system.native_contract_provider().ok_or_else(|| {
            neo_error::CoreError::invalid_operation(
                "persist_block_sequence requires a native-contract provider from SystemContext",
            )
        })?;
        Ok(Some(BatchPersistResources {
            snapshot,
            settings: self.system.settings(),
            native_persist: crate::native_persist::NativePersistResources::from_provider(
                native_contract_provider,
            ),
        }))
    }

    pub(crate) fn persist_block_sequence_with_resources(
        &self,
        block: Arc<Block>,
        options: crate::native_persist::NativePersistOptions,
        persist_context: BlockPersistContext,
        resources: &BatchPersistResources<S::NativeProvider, S::CacheBacking>,
    ) -> bool {
        match crate::native_persist::stage_block_natives_with_resources(
            Arc::clone(&resources.snapshot),
            Arc::clone(&block),
            resources.settings.as_ref(),
            options,
            &resources.native_persist,
        ) {
            Ok(staged) => {
                if !self.system.block_committing_with_context(
                    block.as_ref(),
                    staged.snapshot(),
                    &staged.outcome.application_executed,
                    persist_context,
                ) {
                    error!(
                        target: "neo",
                        index = block.index(),
                        "block committing hook failed"
                    );
                    return false;
                }
                staged.commit();

                debug!(
                    target: "neo",
                    initialized = ?staged.outcome.initialized,
                    engines = staged.outcome.application_executed.len(),
                    "block persistence pipeline completed"
                );
                true
            }
            Err(err) => {
                error!(target: "neo", %err, "block persistence pipeline failed");
                false
            }
        }
    }
}
