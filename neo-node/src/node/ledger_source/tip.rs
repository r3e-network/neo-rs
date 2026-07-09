//! Local persisted-ledger tip helpers.
//!
//! This module is the node-local provider boundary for operational reads of
//! the durable ledger pointer. Startup, config validation, chain.acc resume, and
//! daemon system context all route through this helper so the no-cold-archive
//! case and future static-file cold archive share one provider shape.

use std::sync::Arc;

use neo_blockchain::{
    ChainTipProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory, LedgerProviderFactory,
};
use neo_primitives::UInt256;
use neo_storage::persistence::{DataCache, StoreCache, store::Store};

const LOCAL_LEDGER_TIP_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::node) struct LocalLedgerTip {
    pub(in crate::node) height: u32,
    pub(in crate::node) hash: UInt256,
}

/// Reads the current ledger index from an existing snapshot.
pub(in crate::node) fn snapshot_ledger_index(snapshot: &DataCache) -> Option<u32> {
    LOCAL_LEDGER_TIP_PROVIDER_FACTORY
        .provider(snapshot)
        .current_index()
        .ok()
}

/// Reads the current ledger index from a fresh store-backed snapshot.
pub(in crate::node) fn store_ledger_index(store: &Arc<dyn Store>, read_only: bool) -> Option<u32> {
    let cache = StoreCache::new_from_store(Arc::clone(store), read_only);
    snapshot_ledger_index(cache.data_cache())
}

/// Reads the current ledger hash and height from a fresh store-backed snapshot.
pub(in crate::node) fn local_ledger_tip(
    store: Option<&Arc<dyn Store>>,
) -> anyhow::Result<Option<LocalLedgerTip>> {
    let Some(store) = store else {
        return Ok(None);
    };
    let cache = StoreCache::new_from_store(Arc::clone(store), true);
    snapshot_ledger_tip(cache.data_cache())
}

fn snapshot_ledger_tip(snapshot: &DataCache) -> anyhow::Result<Option<LocalLedgerTip>> {
    let provider = LOCAL_LEDGER_TIP_PROVIDER_FACTORY.provider(snapshot);
    let Ok(height) = provider.current_index() else {
        return Ok(None);
    };
    let hash = provider
        .current_hash()
        .map_err(|err| anyhow::anyhow!("reading local persisted ledger tip hash: {err}"))?;
    Ok(Some(LocalLedgerTip { height, hash }))
}
