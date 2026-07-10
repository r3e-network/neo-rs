use std::sync::Arc;

use neo_payloads::Block;
use tracing::warn;

use crate::block_processing::BatchPersistResources;
use crate::service::{BlockchainService, MempoolLike};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Verify an import-command block with either the shared bulk-sync
    /// persistence resources or a fresh live snapshot.
    ///
    /// The verified import pipeline remains the canonical validation path.
    /// This helper only keeps resource selection and import-command logging out
    /// of the main block loop.
    pub(crate) fn verify_import_block_for_command(
        &self,
        block: &Block,
        current_height: u32,
        bulk_sync: bool,
        batch_persist_resources: Option<&BatchPersistResources<S::NativeProvider, S::CacheBacking>>,
    ) -> bool {
        let verify_result = if let Some(resources) = batch_persist_resources {
            self.verify_import_block_with_pipeline(
                block,
                current_height,
                bulk_sync,
                Arc::clone(&resources.settings),
                Arc::clone(&resources.snapshot),
                resources.native_persist.provider(),
            )
        } else {
            let snapshot = match self.system.store_snapshot() {
                Some(snapshot) => snapshot,
                None => {
                    warn!(
                        target: "neo",
                        height = block.index(),
                        "import aborted: store snapshot unavailable for block validation"
                    );
                    return false;
                }
            };
            self.verify_import_block_with_pipeline(
                block,
                current_height,
                bulk_sync,
                self.system.settings(),
                snapshot,
                match self.system.native_contract_provider() {
                    Some(provider) => provider,
                    None => {
                        warn!(
                            target: "neo",
                            height = block.index(),
                            "import aborted: native contract provider unavailable for block validation"
                        );
                        return false;
                    }
                },
            )
        };

        if let Err(error) = verify_result {
            warn!(
                target: "neo",
                %error,
                height = block.index(),
                "import aborted: block verification failed"
            );
            return false;
        }

        true
    }
}
