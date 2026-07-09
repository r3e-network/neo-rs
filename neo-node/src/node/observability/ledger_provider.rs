//! Ledger-height resolution shared by observability surfaces.
//!
//! Health payloads, readiness responses, and Prometheus metrics all need the
//! same view of the node tip: remote-ledger mode should report the advertised
//! upstream height, while local-ledger mode should read the persisted Ledger
//! current index. This module keeps that policy in one provider boundary.

use neo_blockchain::{
    ChainTipProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory, LedgerProviderFactory,
};

use super::super::remote_ledger::RemoteLedgerStatus;

const OBSERVABILITY_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

/// Returns the observability-facing ledger height for `node`.
pub(in crate::node) fn observability_ledger_height(node: &neo_system::Node) -> Option<u32> {
    if let Some(remote_ledger) = node.get_service::<RemoteLedgerStatus>() {
        return remote_ledger.advertised_height;
    }
    let cache = node.store_cache();
    OBSERVABILITY_LEDGER_PROVIDER_FACTORY
        .provider(cache.data_cache())
        .current_index()
        .ok()
}
