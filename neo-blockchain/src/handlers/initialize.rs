use std::sync::Arc;

use tracing::debug;

use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::Initialize`] command.
    ///
    /// C# `Blockchain.OnInitialize` (Blockchain.cs:197): when the chain state is
    /// uninitialized (`!NativeContract.Ledger.Initialized`), persist the genesis
    /// block, which deploys/initializes the genesis-active natives (NEO committee
    /// cache + total-supply mint, Oracle price, ...) and runs their
    /// OnPersist/PostPersist hooks. Without a store snapshot from the
    /// [`SystemContext`] the service cannot persist genesis and therefore leaves
    /// initialization to the caller.
    pub(crate) async fn initialize(&self) -> Result<(), String> {
        let Some(snapshot) = self.system.store_snapshot() else {
            debug!(target: "neo", "blockchain service initialized without a store snapshot");
            return Ok(());
        };
        if crate::native_persist::chain_state_initialized(&snapshot) {
            debug!(
                target: "neo",
                height = self.ledger.current_height(),
                "blockchain service already initialized"
            );
            return Ok(());
        }

        let settings = self.system.settings();
        let genesis = Arc::new(
            crate::native_persist::genesis_block(settings.as_ref())
                .map_err(|error| format!("genesis block construction failed: {error}"))?,
        );
        let genesis_hash = genesis
            .try_hash()
            .map_err(|error| format!("genesis block hash failed: {error}"))?;
        let native_persist = self.system.native_persist_resources().ok_or_else(|| {
            "genesis native persistence requires native persistence resources from SystemContext"
                .to_string()
        })?;
        let staged = crate::native_persist::stage_block_natives_with_resources(
            Arc::clone(&snapshot),
            Arc::clone(&genesis),
            Arc::clone(&settings),
            crate::native_persist::NativePersistOptions::default(),
            &native_persist,
        )
        .map_err(|error| format!("genesis persistence failed: {error}"))?;
        if !self.system.block_committing(
            genesis.as_ref(),
            staged.snapshot(),
            &staged.outcome.application_executed,
        ) {
            return Err("genesis committing hook failed".to_string());
        }

        staged.commit();
        let application_executed = staged.outcome.application_executed;
        self.system
            .commit_to_store()
            .map_err(|error| format!("genesis durable store commit failed: {error}"))?;
        self.ledger
            .insert_block_arc_with_hash(Arc::clone(&genesis), genesis_hash);
        self.system
            .block_finalized(crate::FinalizedBlock::new(
                Arc::clone(&genesis),
                Some(snapshot),
                application_executed,
                BlockPersistContext::live(),
            ))
            .await
            .map_err(|error| {
                format!("genesis committed durably but finalized delivery failed: {error}")
            })?;
        if self.system.should_stop_blockchain_service() {
            return Err(
                "genesis committed durably but canonical writer shutdown was requested".to_string(),
            );
        }
        debug!(
            target: "neo",
            height = self.ledger.current_height(),
            "blockchain service initialized"
        );
        Ok(())
    }
}
