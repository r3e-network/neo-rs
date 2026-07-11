//! Static Ledger archive startup and hot-prefix reconciliation.

use std::sync::Arc;

use anyhow::Context;
use neo_storage::persistence::{StoreCache, providers::RuntimeStore};
use tracing::info;

use super::config::NodeConfig;

/// Maximum finalized heights retained in memory before one archive publish.
///
/// P2P download batches are capped to this value. Other import sources that
/// submit a larger range fall back to per-block durability through the commit
/// hook policy, preventing unbounded cloned Ledger rows.
pub(super) const STATIC_ARCHIVE_MAX_DEFERRED_BLOCKS: usize = 64;

/// Opens the configured archive and reconciles it to authoritative MDBX or
/// RocksDB Ledger records before any node read service is exposed.
pub(super) async fn open_static_ledger_archive(
    config: &NodeConfig,
    store: &Arc<RuntimeStore>,
    canonical_tip: Option<u32>,
) -> anyhow::Result<Option<neo_blockchain::StaticLedgerArchive>> {
    let Some(path) = config.storage.static_file_path() else {
        return Ok(None);
    };
    let static_config = config.storage.static_file_config();
    let recovery_batch_blocks = config.storage.static_file_recovery_batch_blocks();
    let store = Arc::clone(store);
    let worker_path = path.clone();
    let (archive, recovery) = tokio::task::spawn_blocking(move || {
        let archive = neo_blockchain::StaticLedgerArchiveFactory::new(static_config)
            .open(&worker_path)
            .with_context(|| format!("opening static Ledger archive {}", worker_path.display()))?;
        let hot = StoreCache::new_from_store(store, true);
        let recovery = archive
            .reconcile(hot.data_cache(), canonical_tip, recovery_batch_blocks)
            .with_context(|| {
                format!(
                    "reconciling static Ledger archive {}",
                    worker_path.display()
                )
            })?;
        Ok::<_, anyhow::Error>((archive, recovery))
    })
    .await
    .context("static Ledger archive recovery worker failed")??;
    info!(
        target: "neo::static_files",
        path = %path.display(),
        canonical_tip,
        archive_tip = recovery.final_tip,
        appended_blocks = recovery.appended_blocks,
        truncated_blocks = recovery.truncated_blocks,
        "static Ledger archive is reconciled"
    );
    Ok(Some(archive))
}
