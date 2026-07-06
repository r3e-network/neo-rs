use std::sync::Arc;

use tracing::{debug, warn};

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
    pub(crate) async fn initialize(&self) {
        if let Some(snapshot) = self.system.store_snapshot() {
            if !crate::native_persist::chain_state_initialized(&snapshot) {
                let settings = self.system.settings();
                match crate::native_persist::genesis_block(settings.as_ref()) {
                    Ok(genesis) => {
                        let genesis = Arc::new(genesis);
                        let Some(native_contract_provider) = self.system.native_contract_provider()
                        else {
                            tracing::error!(
                                target: "neo",
                                "genesis native persistence requires a native-contract provider from SystemContext"
                            );
                            return;
                        };
                        let native_persist =
                            crate::native_persist::NativePersistResources::from_provider(
                                native_contract_provider,
                            );
                        match crate::native_persist::stage_block_natives_with_resources(
                            Arc::clone(&snapshot),
                            Arc::clone(&genesis),
                            settings.as_ref(),
                            crate::native_persist::NativePersistOptions::default(),
                            &native_persist,
                        ) {
                            Ok(staged) => {
                                if !self.system.block_committing(
                                    genesis.as_ref(),
                                    staged.snapshot(),
                                    &staged.outcome.application_executed,
                                ) {
                                    tracing::error!(
                                        target: "neo",
                                        index = genesis.index(),
                                        "genesis committing hook failed"
                                    );
                                    return;
                                }
                                staged.commit();
                                if let Err(error) =
                                    self.ledger.insert_block_arc(Arc::clone(&genesis))
                                {
                                    warn!(
                                        target: "neo",
                                        %error,
                                        "failed to record the genesis block in the ledger cache"
                                    );
                                }
                                // Flush genesis through to the durable store so a
                                // fresh node persists it on disk, not just in memory.
                                self.system.commit_to_store();
                                self.system.block_committed_with_context(
                                    genesis.as_ref(),
                                    BlockPersistContext::live(),
                                );
                                debug!(
                                    target: "neo",
                                    initialized = ?staged.outcome.initialized,
                                    "genesis block persisted"
                                );
                            }
                            Err(error) => {
                                tracing::error!(
                                    target: "neo",
                                    %error,
                                    "genesis persistence failed"
                                );
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!(
                            target: "neo",
                            %error,
                            "genesis block construction failed"
                        );
                    }
                }
            }
        }
        debug!(
            target: "neo",
            height = self.ledger.current_height(),
            "blockchain service initialized"
        );
    }
}
