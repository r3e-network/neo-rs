//! Concrete finalized projection consumer.

use std::sync::Arc;

use neo_blockchain::FinalizedBlock;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_runtime::FinalizedHandler;
use neo_storage::CacheRead;
use neo_storage::persistence::store::Store;
use neo_system::FinalizedBlockConsumer;

/// Optional node-local projections derived from one finalized notification.
pub(in crate::node) struct FinalizedProjectionConsumer<P, L, T>
where
    P: NativeContractProvider,
    L: Store,
    T: Store,
{
    network: u32,
    application_logs: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<L>>>,
    tokens_tracker: Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker<P, T>>>,
}

impl<P, L, T> FinalizedProjectionConsumer<P, L, T>
where
    P: NativeContractProvider + 'static,
    L: Store + 'static,
    T: Store + 'static,
{
    pub(in crate::node) fn new(
        network: u32,
        application_logs: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<L>>>,
        tokens_tracker: Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker<P, T>>>,
    ) -> Self {
        Self {
            network,
            application_logs,
            tokens_tracker,
        }
    }

    pub(in crate::node) const fn has_consumers(&self) -> bool {
        self.application_logs.is_some() || self.tokens_tracker.is_some()
    }
}

impl<P, L, T> std::fmt::Debug for FinalizedProjectionConsumer<P, L, T>
where
    P: NativeContractProvider,
    L: Store,
    T: Store,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FinalizedProjectionConsumer")
            .field("network", &self.network)
            .field("application_logs", &self.application_logs.is_some())
            .field("tokens_tracker", &self.tokens_tracker.is_some())
            .finish()
    }
}

impl<P, L, T, B> FinalizedBlockConsumer<B> for FinalizedProjectionConsumer<P, L, T>
where
    P: NativeContractProvider + 'static,
    L: Store + 'static,
    T: Store + 'static,
    B: CacheRead,
{
    fn consume(&self, finalized: &FinalizedBlock<B>) -> Result<(), String> {
        if !self.has_consumers() || finalized.context().skips_live_observers() {
            return Ok(());
        }
        let snapshot = finalized.snapshot().ok_or_else(|| {
            format!(
                "finalized block {} has no canonical snapshot for read projections",
                finalized.block().index()
            )
        })?;
        if let Some(application_logs) = &self.application_logs {
            application_logs.blockchain_finalized_handler(
                self.network,
                finalized.block().as_ref(),
                snapshot.as_ref(),
                finalized.application_executed(),
            );
        }
        if let Some(tokens_tracker) = &self.tokens_tracker {
            tokens_tracker.blockchain_finalized_handler(
                self.network,
                finalized.block().as_ref(),
                snapshot.as_ref(),
                finalized.application_executed(),
            );
        }
        Ok(())
    }
}
