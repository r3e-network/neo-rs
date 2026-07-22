use std::sync::Arc;

use neo_config::NeoChainSpec;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block};
use neo_storage::{CacheRead, DataCache};
use tracing::{debug, error};

use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

/// Execution artifacts retained until canonical durability permits publication.
pub(crate) struct BlockCommitArtifacts<B>
where
    B: CacheRead,
{
    snapshot: Option<Arc<DataCache<B>>>,
    application_executed: Vec<ApplicationExecuted>,
}

impl<B> BlockCommitArtifacts<B>
where
    B: CacheRead,
{
    pub(crate) fn storeless() -> Self {
        Self {
            snapshot: None,
            application_executed: Vec::new(),
        }
    }

    pub(crate) fn without_replay_artifacts(snapshot: Option<Arc<DataCache<B>>>) -> Self {
        Self {
            snapshot,
            application_executed: Vec::new(),
        }
    }

    pub(crate) fn into_finalized(
        self,
        block: Arc<Block>,
        context: BlockPersistContext,
    ) -> crate::FinalizedBlock<B> {
        crate::FinalizedBlock::new(block, self.snapshot, self.application_executed, context)
    }
}

pub(crate) struct BatchPersistResources<P, B>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
{
    pub(crate) snapshot: Arc<DataCache<B>>,
    pub(crate) chain_spec: Arc<NeoChainSpec>,
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
    pub(crate) async fn persist_block_sequence_with_options(
        &self,
        block: Arc<Block>,
        options: crate::native_persist::NativePersistOptions,
    ) -> Result<BlockCommitArtifacts<S::CacheBacking>, String> {
        let persist_context = if options.capture_replay_artifacts {
            BlockPersistContext::live()
        } else {
            BlockPersistContext::trusted_replay()
        };
        self.persist_block_sequence_with_context(block, options, persist_context)
            .await
    }

    pub(crate) async fn persist_block_sequence_with_context(
        &self,
        block: Arc<Block>,
        options: crate::native_persist::NativePersistOptions,
        persist_context: BlockPersistContext,
    ) -> Result<BlockCommitArtifacts<S::CacheBacking>, String> {
        let resources = match self.batch_persist_resources(block.index()) {
            Ok(Some(resources)) => resources,
            Ok(None) => return Ok(BlockCommitArtifacts::storeless()),
            Err(err) => {
                error!(
                    target: "neo",
                    %err,
                    "block persistence pipeline resource setup failed"
                );
                return Err(err.to_string());
            }
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
        let native_persist = self.system.native_persist_resources().ok_or_else(|| {
            neo_error::CoreError::invalid_operation(
                "persist_block_sequence requires native persistence resources from SystemContext",
            )
        })?;
        Ok(Some(BatchPersistResources {
            snapshot,
            chain_spec: self.system.chain_spec(),
            native_persist,
        }))
    }

    pub(crate) fn persist_block_sequence_with_resources(
        &self,
        block: Arc<Block>,
        options: crate::native_persist::NativePersistOptions,
        persist_context: BlockPersistContext,
        resources: &BatchPersistResources<S::NativeProvider, S::CacheBacking>,
    ) -> Result<BlockCommitArtifacts<S::CacheBacking>, String> {
        match crate::native_persist::stage_block_natives_with_resources(
            Arc::clone(&resources.snapshot),
            Arc::clone(&block),
            resources.chain_spec.protocol_settings_arc(),
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
                    return Err("block committing hook failed".to_string());
                }
                staged.commit();

                let application_executed = staged.outcome.application_executed;

                debug!(
                    target: "neo",
                    initialized = ?staged.outcome.initialized,
                    engines = application_executed.len(),
                    "block persistence pipeline completed"
                );
                Ok(BlockCommitArtifacts {
                    snapshot: Some(Arc::clone(&resources.snapshot)),
                    application_executed,
                })
            }
            Err(err) => {
                error!(target: "neo", %err, "block persistence pipeline failed");
                Err(err.to_string())
            }
        }
    }
}
