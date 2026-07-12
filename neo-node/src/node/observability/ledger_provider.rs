//! Ledger-height resolution shared by observability surfaces.
//!
//! Health payloads, readiness responses, and Prometheus metrics all need the
//! same view of the node tip: remote-ledger mode should report the advertised
//! upstream height, while local-ledger mode should read the persisted Ledger
//! current index. This module keeps that policy in one provider boundary.

use neo_blockchain::{
    ChainTipProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory, LedgerProviderFactory,
};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::TransactionalStore;

use super::super::services::NodeServiceHandles;

const OBSERVABILITY_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

/// Returns the observability-facing ledger height for `node`.
pub(in crate::node) fn observability_ledger_height<P, S>(
    node: &neo_system::Node<P, S>,
    services: &NodeServiceHandles<S>,
) -> Option<u32>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    if let Some(remote_ledger) = services.remote_ledger() {
        return remote_ledger.advertised_height;
    }
    let cache = node.store_cache();
    OBSERVABILITY_LEDGER_PROVIDER_FACTORY
        .provider(cache.data_cache())
        .current_index()
        .ok()
}
