//! Static Ledger archive startup, watermark-aware recovery, and hot pruning.

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

/// Maximum archive frames covered by one atomic hot-pruning transaction.
pub(super) const STATIC_ARCHIVE_PRUNE_BATCH_FRAMES: usize = 1_024;

/// Returns the highest height outside the retained traceability window.
pub(super) const fn hot_ledger_prune_target(
    canonical_tip: u32,
    retention_blocks: u32,
) -> Option<u32> {
    canonical_tip.checked_sub(retention_blocks)
}

/// Opens the configured archive, reconciles its still-hot suffix, and resumes
/// bounded hot pruning before any node read service is exposed.
pub(super) async fn open_static_ledger_archive(
    config: &NodeConfig,
    store: &Arc<RuntimeStore>,
    canonical_tip: Option<u32>,
    hot_retention_blocks: u32,
) -> anyhow::Result<Option<neo_blockchain::StaticLedgerArchive>> {
    let Some(path) = config.storage.static_file_path() else {
        return Ok(None);
    };
    let static_config = config.storage.static_file_config();
    let recovery_batch_blocks = config.storage.static_file_recovery_batch_blocks();
    let store = Arc::clone(store);
    let worker_path = path.clone();
    let (archive, recovery, hot_pruned_through, pruned_rows) =
        tokio::task::spawn_blocking(move || {
            let archive = neo_blockchain::StaticLedgerArchiveFactory::new(static_config)
                .open(&worker_path)
                .with_context(|| {
                    format!("opening static Ledger archive {}", worker_path.display())
                })?;
            let hot_pruned_through =
                archive
                    .hot_pruned_through(store.as_ref())
                    .with_context(|| {
                        format!(
                            "reading hot Ledger prune watermark for {}",
                            worker_path.display()
                        )
                    })?;
            let hot = StoreCache::new_from_store(Arc::clone(&store), true);
            let recovery = archive
                .reconcile(
                    hot.data_cache(),
                    canonical_tip,
                    hot_pruned_through,
                    recovery_batch_blocks,
                )
                .with_context(|| {
                    format!(
                        "reconciling static Ledger archive {}",
                        worker_path.display()
                    )
                })?;
            let mut pruned_rows = 0u64;
            if let Some(target) =
                canonical_tip.and_then(|tip| hot_ledger_prune_target(tip, hot_retention_blocks))
            {
                loop {
                    let outcome = archive
                        .prune_hot_through(
                            store.as_ref(),
                            target,
                            STATIC_ARCHIVE_PRUNE_BATCH_FRAMES,
                        )
                        .with_context(|| {
                            format!(
                                "pruning archived hot Ledger rows through {target} for {}",
                                worker_path.display()
                            )
                        })?;
                    pruned_rows = pruned_rows.saturating_add(outcome.deleted_rows);
                    if outcome
                        .pruned_through
                        .is_some_and(|height| height >= target)
                    {
                        break;
                    }
                }
            }
            let hot_pruned_through =
                archive
                    .hot_pruned_through(store.as_ref())
                    .with_context(|| {
                        format!(
                            "reading final hot Ledger prune watermark for {}",
                            worker_path.display()
                        )
                    })?;
            Ok::<_, anyhow::Error>((archive, recovery, hot_pruned_through, pruned_rows))
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
        hot_pruned_through,
        pruned_rows,
        "static Ledger archive is reconciled"
    );
    Ok(Some(archive))
}

#[cfg(test)]
#[path = "../tests/node/static_files.rs"]
mod tests;
