//! Runtime glue binding the extracted `neo-state-service` crate into the
//! neo-system handler registry (Phase 1 S3).
//!
//! The shared handler trait vocabulary (`CommittingHandler`/`CommittedHandler`/
//! `WalletChangedHandler`) stays in neo-system because rpc/oracle/validator/
//! node all implement it. The moved state-service types expose port-typed
//! inherent hooks; these orphan-rule-legal impls (local trait for foreign
//! type) adapt the erased `&dyn Any` system identity to the
//! `StateCommitContext` port and delegate. The persistence dispatch
//! (`neo_system::persistence::commit_participants`) and the AI evidence
//! non-blocking contract are unchanged.

use crate::events::handlers::{CommittedHandler, CommittingHandler, CoreResult};
use crate::neo_system::{NeoSystem, NeoSystemContext};
use neo_state_service::commit_handlers::{StateServiceCommitHandlers, StateServiceError};
use neo_state_service::{StateCommitContext, StateStoreService, ValidatedRootPersisted};
use neo_storage::persistence::DataCache;
use std::sync::Arc;

/// Runtime adapter bridging `NeoSystemContext` to the state-service commit
/// port. The canonical persistence adapter supplies `NeoSystem`, while
/// embedders may supply its context directly; both identities must reach
/// the same registered state store.
enum NeoSystemStateCommitContext<'a> {
    Owned(Arc<NeoSystemContext>),
    Borrowed(&'a NeoSystemContext),
}

impl NeoSystemStateCommitContext<'_> {
    fn context(&self) -> &NeoSystemContext {
        match self {
            Self::Owned(context) => context.as_ref(),
            Self::Borrowed(context) => context,
        }
    }
}

impl StateCommitContext for NeoSystemStateCommitContext<'_> {
    fn state_store(&self) -> Result<Option<Arc<dyn StateStoreService>>, StateServiceError> {
        self.context()
            .state_store_typed()
            .map_err(|error| StateServiceError::new(error.to_string()))
    }

    fn publish_validated_root_persisted(&self, event: ValidatedRootPersisted) {
        self.context().event_stream().publish(event);
    }
}

/// Resolves the commit port from the erased system identity passed by the
/// blockchain handler registry.
fn resolve_commit_context(system: &dyn std::any::Any) -> Option<NeoSystemStateCommitContext<'_>> {
    if let Some(system) = system.downcast_ref::<NeoSystem>() {
        return Some(NeoSystemStateCommitContext::Owned(system.context()));
    }
    system
        .downcast_ref::<NeoSystemContext>()
        .map(NeoSystemStateCommitContext::Borrowed)
}

// Implement CommittingHandler trait (runtime glue over the port-typed
// inherent hooks; the trait vocabulary stays in neo-system).
impl CommittingHandler for StateServiceCommitHandlers {
    fn run_during_fast_sync(&self) -> bool {
        StateServiceCommitHandlers::run_during_fast_sync(self)
    }

    fn blockchain_committing_handler(
        &self,
        system: &dyn std::any::Any,
        block: &neo_protocol::execution::Block,
        snapshot: &DataCache,
        application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) {
        let context = resolve_commit_context(system);
        StateServiceCommitHandlers::blockchain_committing_handler(
            self,
            context.as_ref().map(|c| c as &dyn StateCommitContext),
            block,
            snapshot,
            application_executed_list,
        );
    }

    fn try_blockchain_committing_handler(
        &self,
        system: &dyn std::any::Any,
        block: &neo_protocol::execution::Block,
        snapshot: &DataCache,
        application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) -> CoreResult<()> {
        let context = resolve_commit_context(system);
        StateServiceCommitHandlers::try_blockchain_committing_handler(
            self,
            context.as_ref().map(|c| c as &dyn StateCommitContext),
            block,
            snapshot,
            application_executed_list,
        )
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }

    fn blockchain_rollback_handler(
        &self,
        system: &dyn std::any::Any,
        block: &neo_protocol::execution::Block,
        snapshot: &DataCache,
    ) -> CoreResult<()> {
        let context = resolve_commit_context(system);
        StateServiceCommitHandlers::blockchain_rollback_handler(
            self,
            context.as_ref().map(|c| c as &dyn StateCommitContext),
            block,
            snapshot,
        )
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
    }
}

// Implement CommittedHandler trait (runtime glue over the port-typed hook).
impl CommittedHandler for StateServiceCommitHandlers {
    fn blockchain_committed_handler(
        &self,
        system: &dyn std::any::Any,
        block: &neo_protocol::execution::Block,
    ) {
        let context = resolve_commit_context(system);
        StateServiceCommitHandlers::blockchain_committed_handler(
            self,
            context.as_ref().map(|c| c as &dyn StateCommitContext),
            block,
        );
    }
}

// Task 208 / Phase 1 S3: the verification service moved to neo-state-service;
// the wallet-changed registry hook stays here as glue over the inherent
// `on_wallet_changed` hook.
#[cfg(feature = "state-verifier")]
impl crate::events::handlers::WalletChangedHandler
    for neo_state_service::state_verification::StateServiceVerification
{
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn neo_wallets::Wallet>>,
    ) {
        self.on_wallet_changed(wallet);
    }
}
