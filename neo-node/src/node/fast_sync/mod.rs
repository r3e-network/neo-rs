//! # neo-node::node::fast_sync
//!
//! Built-in fast-sync package discovery, download, verification, and import
//! flow.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `package`: Fast-sync package metadata, cache, and archive helpers.
//! - `reference`: Reference RPC verification helpers for fast-sync imports.

use super::config::NodeConfig;
use anyhow::Context;
use neo_blockchain::BlockchainHandle;
use neo_primitives::UInt256;
use neo_state_service::StateStore;
use neo_state_service::commit_handlers::StateServiceCommitHandlers;
use neo_storage::persistence::store::Store;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

mod package;
mod reference;

use package::{
    FastSyncPackage, ensure_chain_acc_extracted, ensure_package_cached, fetch_latest_package,
};

const FAST_SYNC_TARGET_MIN_BPS: f64 = 1500.0;
const FAST_SYNC_TARGET_MAX_BPS: f64 = 2000.0;

pub(super) async fn run_fast_sync_report(
    blockchain: &BlockchainHandle,
    storage: Arc<dyn Store>,
    config: &NodeConfig,
    storage_override: Option<&Path>,
    cache_dir_override: Option<&Path>,
    network: u32,
    stop_at_height: Option<u32>,
    reference_rpc: Option<&str>,
    state_store: Option<&Arc<StateStore>>,
    state_service: Option<&Arc<StateServiceCommitHandlers>>,
) -> anyhow::Result<FastSyncReport> {
    let package = fetch_latest_package(network).await?;
    validate_fast_sync_preflight(&storage, &package)?;
    let cache_dir = fast_sync_cache_dir(config, storage_override, cache_dir_override);
    let zip_path = ensure_package_cached(&package, &cache_dir).await?;
    let chain_path = ensure_chain_acc_extracted(&zip_path, &cache_dir, &package.md5)?;
    refuse_stale_fast_sync_import_marker(&cache_dir)?;
    let import_marker = write_fast_sync_import_marker(&cache_dir, &package, &chain_path)?;
    info!(
        target: "neo::fast_sync",
        network = package.network_key,
        start = package.start,
        end = package.end,
        package = %zip_path.display(),
        chain = %chain_path.display(),
        "importing fast-sync package"
    );
    let report = super::chain_acc::import_chain_acc_report_with_expected_range(
        blockchain,
        &chain_path,
        false,
        super::chain_acc::ChainAccExpectedRange {
            start_height: package.start,
            end_height: package.end,
        },
        stop_at_height,
        Some(Arc::clone(&storage)),
    )
    .await?;
    log_fast_sync_throughput(&package, &report);
    verify_fast_sync_import_tip(&storage, &package, &report)?;
    if let Some(state_service) = state_service {
        state_service.flush_result().map_err(|err| {
            anyhow::anyhow!("state service MPT worker failed after fast-sync import: {err}")
        })?;
    }
    let local_root = match report.last_imported_tip {
        Some(imported_tip) => local_state_root_tip(state_store, &package, imported_tip)?,
        None => None,
    };
    let mut reference_report = None;
    if let Some(endpoint) = reference_rpc {
        if let Some(imported_tip) = report.last_imported_tip {
            let block = reference::verify_block_tip(endpoint, &package, imported_tip).await?;
            let state_root = match local_root {
                Some(local_root) => {
                    Some(reference::verify_state_root_tip(endpoint, &package, local_root).await?)
                }
                None => None,
            };
            reference_report = Some(FastSyncReferenceReport::from_proofs(
                endpoint, block, state_root,
            ));
        }
    }
    clear_fast_sync_import_marker(&import_marker)?;
    Ok(FastSyncReport::from_parts(
        &package,
        &zip_path,
        &chain_path,
        report,
        reference_report,
    ))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct FastSyncReport {
    pub(super) package: FastSyncPackageReport,
    pub(super) import: FastSyncImportReport,
    pub(super) hot_metrics: FastSyncHotMetricsReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reference: Option<FastSyncReferenceReport>,
}

impl FastSyncReport {
    fn from_parts(
        package: &FastSyncPackage,
        zip_path: &Path,
        chain_path: &Path,
        import: super::chain_acc::ChainAccImportReport,
        reference: Option<FastSyncReferenceReport>,
    ) -> Self {
        let hot_metrics = import.hot_metrics;
        Self {
            package: FastSyncPackageReport {
                network: package.network_key.to_string(),
                url: package.url.clone(),
                md5: package.md5.clone(),
                start_height: package.start,
                end_height: package.end,
                filename: package.filename.clone(),
                zip_path: zip_path.display().to_string(),
                chain_path: chain_path.display().to_string(),
            },
            import: FastSyncImportReport {
                imported_blocks: import.imported,
                final_height: import.last_imported_tip.map(|tip| tip.height),
                final_hash: import.last_imported_tip.map(|tip| tip.hash.to_string()),
                elapsed_seconds: import.elapsed_seconds,
                average_blocks_per_second: import.average_blocks_per_second,
                empty_blocks: import.empty_blocks,
                empty_only_blocks: import.empty_only_blocks,
                empty_block_import_seconds: import.empty_block_import_seconds,
                empty_blocks_per_second: import.empty_blocks_per_second,
                transaction_blocks: import.transaction_blocks,
                transactions: import.transactions,
                transaction_block_import_seconds: import.transaction_block_import_seconds,
                transaction_blocks_per_second: import.transaction_blocks_per_second,
                throughput_status: fast_sync_throughput_status(
                    import.imported,
                    import.average_blocks_per_second,
                ),
            },
            hot_metrics: FastSyncHotMetricsReport {
                state_service_mpt_avg_total_us: hot_metrics.state_service_mpt_avg_total_us,
                state_service_mpt_trie_commit_avg_us: hot_metrics
                    .state_service_mpt_trie_commit_avg_us,
                native_persist_avg_total_us: hot_metrics.native_persist_avg_total_us,
                native_persist_tx_hot_stage: hot_metrics.native_persist_tx_hot_stage.to_string(),
                native_persist_tx_hot_stage_avg_us: hot_metrics.native_persist_tx_hot_stage_avg_us,
                rocksdb_batch_avg_flush_duration_ms: hot_metrics
                    .rocksdb_batch_avg_flush_duration_ms,
                rocksdb_batch_pending_operations: hot_metrics.rocksdb_batch_pending_operations,
            },
            reference,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct FastSyncPackageReport {
    pub(super) network: String,
    pub(super) url: String,
    pub(super) md5: String,
    pub(super) start_height: u32,
    pub(super) end_height: u32,
    pub(super) filename: String,
    pub(super) zip_path: String,
    pub(super) chain_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(super) struct FastSyncImportReport {
    pub(super) imported_blocks: u64,
    pub(super) final_height: Option<u32>,
    pub(super) final_hash: Option<String>,
    pub(super) elapsed_seconds: f64,
    pub(super) average_blocks_per_second: f64,
    pub(super) empty_blocks: u64,
    pub(super) empty_only_blocks: u64,
    pub(super) empty_block_import_seconds: f64,
    pub(super) empty_blocks_per_second: f64,
    pub(super) transaction_blocks: u64,
    pub(super) transactions: u64,
    pub(super) transaction_block_import_seconds: f64,
    pub(super) transaction_blocks_per_second: f64,
    pub(super) throughput_status: FastSyncThroughputStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct FastSyncHotMetricsReport {
    pub(super) state_service_mpt_avg_total_us: u64,
    pub(super) state_service_mpt_trie_commit_avg_us: u64,
    pub(super) native_persist_avg_total_us: u64,
    pub(super) native_persist_tx_hot_stage: String,
    pub(super) native_persist_tx_hot_stage_avg_us: u64,
    pub(super) rocksdb_batch_avg_flush_duration_ms: u64,
    pub(super) rocksdb_batch_pending_operations: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FastSyncBlockReferenceProof {
    pub(super) height: u32,
    pub(super) hash: UInt256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FastSyncStateRootReferenceProof {
    pub(super) height: u32,
    pub(super) root_hash: UInt256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct FastSyncReferenceReport {
    pub(super) endpoint: String,
    pub(super) block_height: u32,
    pub(super) block_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) state_root_height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) state_root_hash: Option<String>,
}

impl FastSyncReferenceReport {
    fn from_proofs(
        endpoint: &str,
        block: FastSyncBlockReferenceProof,
        state_root: Option<FastSyncStateRootReferenceProof>,
    ) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            block_height: block.height,
            block_hash: block.hash.to_string(),
            state_root_height: state_root.map(|proof| proof.height),
            state_root_hash: state_root.map(|proof| proof.root_hash.to_string()),
        }
    }
}

pub(super) fn write_fast_sync_report_sidecar(
    path: &Path,
    report: &FastSyncReport,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating fast-sync report directory {}", parent.display()))?;
    }
    let payload = serde_json::to_vec_pretty(report).context("serializing fast-sync report")?;
    std::fs::write(path, payload)
        .with_context(|| format!("writing fast-sync report {}", path.display()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum FastSyncThroughputStatus {
    NoImport,
    BelowTarget,
    WithinTarget,
    AboveTarget,
}

pub(super) fn fast_sync_throughput_status(
    imported: u64,
    average_blocks_per_second: f64,
) -> FastSyncThroughputStatus {
    if imported == 0 {
        return FastSyncThroughputStatus::NoImport;
    }
    if average_blocks_per_second < FAST_SYNC_TARGET_MIN_BPS {
        FastSyncThroughputStatus::BelowTarget
    } else if average_blocks_per_second > FAST_SYNC_TARGET_MAX_BPS {
        FastSyncThroughputStatus::AboveTarget
    } else {
        FastSyncThroughputStatus::WithinTarget
    }
}

fn log_fast_sync_throughput(
    package: &FastSyncPackage,
    report: &super::chain_acc::ChainAccImportReport,
) {
    let status = fast_sync_throughput_status(report.imported, report.average_blocks_per_second);
    match status {
        FastSyncThroughputStatus::BelowTarget => warn!(
            target: "neo::fast_sync",
            package = %package.filename,
            imported = report.imported,
            elapsed_seconds = report.elapsed_seconds,
            average_blocks_per_second = report.average_blocks_per_second,
            target_min_bps = FAST_SYNC_TARGET_MIN_BPS,
            target_max_bps = FAST_SYNC_TARGET_MAX_BPS,
            "fast-sync package import finished below target throughput"
        ),
        FastSyncThroughputStatus::NoImport => info!(
            target: "neo::fast_sync",
            package = %package.filename,
            imported = report.imported,
            elapsed_seconds = report.elapsed_seconds,
            average_blocks_per_second = report.average_blocks_per_second,
            target_min_bps = FAST_SYNC_TARGET_MIN_BPS,
            target_max_bps = FAST_SYNC_TARGET_MAX_BPS,
            "fast-sync package import skipped because local ledger already covers requested range"
        ),
        FastSyncThroughputStatus::WithinTarget | FastSyncThroughputStatus::AboveTarget => info!(
            target: "neo::fast_sync",
            package = %package.filename,
            imported = report.imported,
            elapsed_seconds = report.elapsed_seconds,
            average_blocks_per_second = report.average_blocks_per_second,
            target_min_bps = FAST_SYNC_TARGET_MIN_BPS,
            target_max_bps = FAST_SYNC_TARGET_MAX_BPS,
            status = ?status,
            "fast-sync package import throughput summary"
        ),
    }
}

fn validate_fast_sync_preflight(
    store: &Arc<dyn Store>,
    package: &FastSyncPackage,
) -> anyhow::Result<()> {
    let durable_tip = super::chain_acc::local_ledger_tip(Some(store))?.map(|tip| tip.height);
    match package.start.checked_sub(1) {
        None => match durable_tip {
            None | Some(0) => Ok(()),
            Some(tip) if tip < package.end => Ok(()),
            Some(tip) if tip == package.end => Ok(()),
            Some(tip) => anyhow::bail!(
                "fast sync package {} ends at height {}, but local ledger is already at height {tip}; refusing to import over newer chain data",
                package.filename,
                package.end
            ),
        },
        Some(expected_tip) => match durable_tip {
            Some(tip) if tip == expected_tip => Ok(()),
            Some(tip) => anyhow::bail!(
                "fast sync package {} starts at height {}, but local ledger tip is {tip}; expected tip {expected_tip} before import",
                package.filename,
                package.start
            ),
            None => anyhow::bail!(
                "fast sync package {} starts at height {}, but local ledger has no tip; expected tip {expected_tip} before import",
                package.filename,
                package.start
            ),
        },
    }
}

fn verify_fast_sync_import_tip(
    store: &Arc<dyn Store>,
    package: &FastSyncPackage,
    report: &super::chain_acc::ChainAccImportReport,
) -> anyhow::Result<()> {
    let Some(imported_tip) = report.last_imported_tip else {
        if report.imported == 0 {
            return Ok(());
        }
        anyhow::bail!(
            "fast-sync package {} imported {} blocks but did not report a final block tip",
            package.filename,
            report.imported
        );
    };

    let durable_tip = super::chain_acc::local_ledger_tip(Some(store))?.ok_or_else(|| {
        anyhow::anyhow!(
            "fast-sync package {} imported to height {}, but local durable ledger has no tip after import",
            package.filename,
            imported_tip.height
        )
    })?;

    if durable_tip != imported_tip {
        anyhow::bail!(
            "fast-sync local ledger tip mismatch after package {}: expected imported tip height {} hash {}, local durable tip height {} hash {}",
            package.filename,
            imported_tip.height,
            imported_tip.hash,
            durable_tip.height,
            durable_tip.hash
        );
    }

    if imported_tip.height > package.end {
        anyhow::bail!(
            "fast-sync package {} imported tip height {} beyond package end {}",
            package.filename,
            imported_tip.height,
            package.end
        );
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LocalStateRootTip {
    pub(super) index: u32,
    pub(super) root_hash: UInt256,
}

fn local_state_root_tip(
    state_store: Option<&Arc<StateStore>>,
    package: &FastSyncPackage,
    imported_tip: super::chain_acc::LocalLedgerTip,
) -> anyhow::Result<Option<LocalStateRootTip>> {
    let Some(state_store) = state_store else {
        return Ok(None);
    };
    let Some(mpt) = state_store.mpt() else {
        return Ok(None);
    };
    let Some((index, root_hash)) = mpt.current_local_root() else {
        anyhow::bail!(
            "fast-sync package {} imported to height {}, but StateService has no local state root",
            package.filename,
            imported_tip.height
        );
    };

    if index != imported_tip.height {
        anyhow::bail!(
            "fast-sync package {} local state-root tip height {} does not match imported block tip height {}",
            package.filename,
            index,
            imported_tip.height
        );
    }

    let state_root = mpt.get_state_root(imported_tip.height).ok_or_else(|| {
        anyhow::anyhow!(
            "fast-sync package {} has no local state root for imported tip height {}",
            package.filename,
            imported_tip.height
        )
    })?;

    if *state_root.root_hash() != root_hash {
        anyhow::bail!(
            "fast-sync package {} local state-root record mismatch at height {}: current root {}, indexed record {}",
            package.filename,
            imported_tip.height,
            root_hash,
            state_root.root_hash()
        );
    }

    Ok(Some(LocalStateRootTip { index, root_hash }))
}

pub(super) fn fast_sync_cache_dir(
    config: &NodeConfig,
    storage_override: Option<&Path>,
    cache_dir_override: Option<&Path>,
) -> PathBuf {
    if let Some(cache_dir) = cache_dir_override {
        return cache_dir.to_path_buf();
    }
    let storage_root = storage_override
        .map(Path::to_path_buf)
        .or_else(|| config.storage.data_directory())
        .unwrap_or_else(|| PathBuf::from("data"));
    storage_root.join("fast-sync")
}

const FAST_SYNC_IMPORT_IN_PROGRESS_MARKER: &str = ".neo-fast-sync-import-in-progress";

fn fast_sync_import_marker_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(FAST_SYNC_IMPORT_IN_PROGRESS_MARKER)
}

fn refuse_stale_fast_sync_import_marker(cache_dir: &Path) -> anyhow::Result<()> {
    let marker_path = fast_sync_import_marker_path(cache_dir);
    if marker_path.exists() {
        anyhow::bail!(
            "previous fast-sync import did not finish cleanly (marker: {}); restore a checkpoint or remove the local ledger before retrying, then remove this marker",
            marker_path.display()
        );
    }
    Ok(())
}

fn write_fast_sync_import_marker(
    cache_dir: &Path,
    package: &FastSyncPackage,
    chain_path: &Path,
) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating fast-sync cache {}", cache_dir.display()))?;
    let marker_path = fast_sync_import_marker_path(cache_dir);
    let content = format!(
        "network={}\nstart={}\nend={}\npackage={}\nchain={}\n",
        package.network_key,
        package.start,
        package.end,
        package.filename,
        chain_path.display()
    );
    std::fs::write(&marker_path, content)
        .with_context(|| format!("writing fast-sync import marker {}", marker_path.display()))?;
    Ok(marker_path)
}

fn clear_fast_sync_import_marker(marker_path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(marker_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err)
            .with_context(|| format!("removing fast-sync import marker {}", marker_path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::chain_acc;
    use serde_json::Value;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn default_cache_dir_tracks_storage_path() {
        let config: NodeConfig = toml::from_str(
            r#"
[storage]
data_dir = "/var/lib/neo/mainnet"
"#,
        )
        .expect("config");

        assert_eq!(
            fast_sync_cache_dir(&config, None, None),
            PathBuf::from("/var/lib/neo/mainnet/fast-sync")
        );
        assert_eq!(
            fast_sync_cache_dir(&config, Some(Path::new("/override")), None),
            PathBuf::from("/override/fast-sync")
        );
        assert_eq!(
            fast_sync_cache_dir(&config, None, Some(Path::new("/cache"))),
            PathBuf::from("/cache")
        );
    }

    fn test_package(start: u32, end: u32) -> FastSyncPackage {
        FastSyncPackage {
            network_key: "n3mainnet",
            url: "https://example.invalid/chain.0.acc.zip".to_string(),
            md5: "ABCDEF0123456789ABCDEF0123456789".to_string(),
            start,
            end,
            filename: format!("chain.{start}.acc.zip"),
        }
    }

    fn memory_store_with_ledger_tip(tip: u32) -> Arc<dyn Store> {
        use neo_storage::persistence::providers::memory_store::MemoryStore;
        use neo_storage::{StorageItem, StorageKey};

        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let mut cache =
            neo_storage::persistence::StoreCache::new_from_store(Arc::clone(&store), false);
        let hash = neo_primitives::UInt256::from([tip as u8; 32]);
        let current = neo_native_contracts::LedgerContract::new()
            .serialize_hash_index_state(&hash, tip)
            .expect("serialize current ledger pointer");
        cache.data_cache().add(
            StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
            StorageItem::from_bytes(current),
        );
        cache.commit();
        store
    }

    fn state_store_with_local_root(
        tip: u32,
    ) -> (Arc<neo_state_service::StateStore>, neo_primitives::UInt256) {
        let state_store = Arc::new(neo_state_service::StateStore::with_mpt(true));
        let mpt = state_store.mpt().expect("MPT store");
        let mut root_before = None;
        for index in 0..=tip {
            let root = mpt
                .apply_block_changes(index, root_before, &[])
                .expect("apply empty MPT changes");
            root_before = Some(root);
        }
        let root_hash = root_before.expect("root applied");
        assert_eq!(mpt.current_local_root(), Some((tip, root_hash)));
        (state_store, root_hash)
    }

    fn import_report(
        imported: u64,
        last_imported_tip: Option<chain_acc::LocalLedgerTip>,
        elapsed_seconds: f64,
        average_blocks_per_second: f64,
    ) -> chain_acc::ChainAccImportReport {
        chain_acc::ChainAccImportReport {
            imported,
            last_imported_tip,
            elapsed_seconds,
            average_blocks_per_second,
            empty_blocks: imported,
            empty_only_blocks: imported,
            empty_block_import_seconds: elapsed_seconds,
            empty_blocks_per_second: average_blocks_per_second,
            transaction_blocks: 0,
            transactions: 0,
            transaction_block_import_seconds: 0.0,
            transaction_blocks_per_second: 0.0,
            hot_metrics: chain_acc::ImportHotMetrics::default(),
        }
    }

    fn import_report_with_composition(
        imported: u64,
        last_imported_tip: Option<chain_acc::LocalLedgerTip>,
        elapsed_seconds: f64,
        average_blocks_per_second: f64,
        empty_blocks: u64,
        transaction_blocks: u64,
        transactions: u64,
    ) -> chain_acc::ChainAccImportReport {
        chain_acc::ChainAccImportReport {
            imported,
            last_imported_tip,
            elapsed_seconds,
            average_blocks_per_second,
            empty_blocks,
            empty_only_blocks: if transaction_blocks > 0 {
                0
            } else {
                empty_blocks
            },
            empty_block_import_seconds: if transaction_blocks > 0 {
                0.0
            } else {
                elapsed_seconds
            },
            empty_blocks_per_second: if transaction_blocks > 0 || elapsed_seconds <= 0.0 {
                0.0
            } else {
                empty_blocks as f64 / elapsed_seconds
            },
            transaction_blocks,
            transactions,
            transaction_block_import_seconds: if transaction_blocks > 0 {
                elapsed_seconds
            } else {
                0.0
            },
            transaction_blocks_per_second: if elapsed_seconds > 0.0 {
                transaction_blocks as f64 / elapsed_seconds
            } else {
                0.0
            },
            hot_metrics: chain_acc::ImportHotMetrics::default(),
        }
    }

    fn serve_rpc_once(expected_method: &'static str, result: Value) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test RPC");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let read = stream.read(&mut buf).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request);
            assert!(
                text.contains(&format!(r#""method":"{expected_method}""#))
                    || text.contains(&format!(r#""method": "{expected_method}""#)),
                "unexpected request: {text}"
            );
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": result,
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });
        url
    }

    #[test]
    fn fast_sync_preflight_allows_full_package_resume_on_existing_ledger() {
        let store = memory_store_with_ledger_tip(42);

        validate_fast_sync_preflight(&store, &test_package(0, 100))
            .expect("full fast-sync package can resume after an existing local tip");
    }

    #[test]
    fn fast_sync_preflight_allows_full_package_already_imported() {
        let store = memory_store_with_ledger_tip(100);

        validate_fast_sync_preflight(&store, &test_package(0, 100))
            .expect("full fast-sync package can be a no-op when local tip is package end");
    }

    #[test]
    fn fast_sync_preflight_rejects_full_package_behind_existing_ledger() {
        let store = memory_store_with_ledger_tip(101);

        let err = validate_fast_sync_preflight(&store, &test_package(0, 100))
            .expect_err("fast sync must not import a package behind the existing local ledger");

        assert!(
            err.to_string()
                .contains("local ledger is already at height 101"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fast_sync_preflight_allows_full_package_on_empty_or_genesis_ledger() {
        use neo_storage::persistence::providers::memory_store::MemoryStore;

        let empty: Arc<dyn Store> = Arc::new(MemoryStore::new());
        validate_fast_sync_preflight(&empty, &test_package(0, 100))
            .expect("empty ledger can import a full fast-sync package");

        let genesis = memory_store_with_ledger_tip(0);
        validate_fast_sync_preflight(&genesis, &test_package(0, 100))
            .expect("genesis-only ledger can import a full fast-sync package");
    }

    #[test]
    fn fast_sync_preflight_requires_previous_tip_for_partial_package() {
        let store = memory_store_with_ledger_tip(9);

        validate_fast_sync_preflight(&store, &test_package(10, 100))
            .expect("partial package can import when local tip is start - 1");

        let err = validate_fast_sync_preflight(&store, &test_package(11, 100))
            .expect_err("partial package must match the local pre-import tip");

        assert!(
            err.to_string().contains("expected tip 10"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn stale_fast_sync_import_marker_blocks_retry() {
        let temp = tempfile::tempdir().expect("temp");
        let marker_path = temp.path().join(".neo-fast-sync-import-in-progress");
        std::fs::write(
            &marker_path,
            "network=n3mainnet\nstart=0\nend=100\npackage=chain.0.acc.zip\n",
        )
        .expect("stale marker");

        let err = refuse_stale_fast_sync_import_marker(temp.path())
            .expect_err("stale in-progress marker should block retry");

        assert!(
            err.to_string()
                .contains("previous fast-sync import did not finish cleanly"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains(&marker_path.display().to_string()),
            "operator error should identify the marker to inspect/remove: {err}"
        );
        assert!(
            err.to_string()
                .contains("restore a checkpoint or remove the local ledger"),
            "operator error should require storage recovery before retry: {err}"
        );
    }

    #[test]
    fn fast_sync_import_marker_records_package_and_is_removed_on_success() {
        let temp = tempfile::tempdir().expect("temp");
        let package = test_package(0, 100);
        let chain_path = temp.path().join("chain.0.acc");
        let marker_path =
            write_fast_sync_import_marker(temp.path(), &package, &chain_path).expect("marker");

        let marker = std::fs::read_to_string(&marker_path).expect("read marker");
        assert!(marker.contains("network=n3mainnet"));
        assert!(marker.contains("start=0"));
        assert!(marker.contains("end=100"));
        assert!(marker.contains("package=chain.0.acc.zip"));
        assert!(marker.contains(&format!("chain={}", chain_path.display())));

        clear_fast_sync_import_marker(&marker_path).expect("clear marker");

        assert!(
            !marker_path.exists(),
            "successful fast-sync import should remove the in-progress marker"
        );
    }

    #[test]
    fn fast_sync_throughput_status_classifies_target_window() {
        assert_eq!(
            fast_sync_throughput_status(0, 0.0),
            FastSyncThroughputStatus::NoImport
        );
        assert_eq!(
            fast_sync_throughput_status(10, 1499.99),
            FastSyncThroughputStatus::BelowTarget
        );
        assert_eq!(
            fast_sync_throughput_status(10, 1500.0),
            FastSyncThroughputStatus::WithinTarget
        );
        assert_eq!(
            fast_sync_throughput_status(10, 2000.0),
            FastSyncThroughputStatus::WithinTarget
        );
        assert_eq!(
            fast_sync_throughput_status(10, 2000.1),
            FastSyncThroughputStatus::AboveTarget
        );
    }

    #[test]
    fn fast_sync_report_preserves_package_and_import_proof() {
        let package = test_package(0, 100);
        let import_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([100; 32]),
        };
        let import = import_report(101, Some(import_tip), 0.0505, 2000.0);
        let report = FastSyncReport::from_parts(
            &package,
            Path::new("/cache/chain.0.acc.zip"),
            Path::new("/cache/chain.0.acc/chain.0.acc"),
            import.with_hot_metrics(chain_acc::ImportHotMetrics {
                state_service_mpt_avg_total_us: 2_000,
                state_service_mpt_trie_commit_avg_us: 1_200,
                native_persist_avg_total_us: 3_000,
                native_persist_tx_hot_stage: "application",
                native_persist_tx_hot_stage_avg_us: 1_700,
                rocksdb_batch_avg_flush_duration_ms: 11,
                rocksdb_batch_pending_operations: 19,
            }),
            None,
        );

        assert_eq!(report.package.network, "n3mainnet");
        assert_eq!(report.package.start_height, 0);
        assert_eq!(report.package.end_height, 100);
        assert_eq!(report.package.filename, "chain.0.acc.zip");
        assert_eq!(report.package.md5, "ABCDEF0123456789ABCDEF0123456789");
        assert_eq!(report.package.zip_path, "/cache/chain.0.acc.zip");
        assert_eq!(report.package.chain_path, "/cache/chain.0.acc/chain.0.acc");
        assert_eq!(report.import.imported_blocks, 101);
        assert_eq!(report.import.final_height, Some(100));
        assert_eq!(report.import.elapsed_seconds, 0.0505);
        assert_eq!(report.import.average_blocks_per_second, 2000.0);
        assert_eq!(report.import.empty_blocks, 101);
        assert_eq!(report.import.empty_only_blocks, 101);
        assert_eq!(report.import.empty_block_import_seconds, 0.0505);
        assert_eq!(report.import.empty_blocks_per_second, 2000.0);
        assert_eq!(report.import.transaction_blocks, 0);
        assert_eq!(report.import.transactions, 0);
        assert_eq!(report.import.transaction_block_import_seconds, 0.0);
        assert_eq!(report.import.transaction_blocks_per_second, 0.0);
        assert_eq!(
            report.import.throughput_status,
            FastSyncThroughputStatus::WithinTarget
        );
        assert_eq!(report.hot_metrics.state_service_mpt_avg_total_us, 2_000);
        assert_eq!(
            report.hot_metrics.state_service_mpt_trie_commit_avg_us,
            1_200
        );
        assert_eq!(report.hot_metrics.native_persist_avg_total_us, 3_000);
        assert_eq!(
            report.hot_metrics.native_persist_tx_hot_stage,
            "application"
        );
        assert_eq!(report.hot_metrics.native_persist_tx_hot_stage_avg_us, 1_700);
        assert_eq!(report.hot_metrics.rocksdb_batch_avg_flush_duration_ms, 11);
        assert_eq!(report.hot_metrics.rocksdb_batch_pending_operations, 19);
    }

    #[test]
    fn write_fast_sync_report_sidecar_serializes_machine_readable_proof() {
        let temp = tempfile::tempdir().expect("temp");
        let package = test_package(0, 100);
        let import_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([100; 32]),
        };
        let report = FastSyncReport::from_parts(
            &package,
            &temp.path().join("chain.0.acc.zip"),
            &temp.path().join("chain.0.acc/chain.0.acc"),
            import_report(101, Some(import_tip), 0.0505, 2000.0).with_hot_metrics(
                chain_acc::ImportHotMetrics {
                    state_service_mpt_avg_total_us: 2_000,
                    state_service_mpt_trie_commit_avg_us: 1_200,
                    native_persist_avg_total_us: 3_000,
                    native_persist_tx_hot_stage: "application",
                    native_persist_tx_hot_stage_avg_us: 1_700,
                    rocksdb_batch_avg_flush_duration_ms: 11,
                    rocksdb_batch_pending_operations: 19,
                },
            ),
            None,
        );
        let path = temp.path().join("proof.json");

        write_fast_sync_report_sidecar(&path, &report).expect("write sidecar");

        let payload: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read sidecar")).expect("json");
        assert_eq!(payload["package"]["network"], "n3mainnet");
        assert_eq!(payload["package"]["start_height"], 0);
        assert_eq!(payload["package"]["end_height"], 100);
        assert_eq!(payload["import"]["imported_blocks"], 101);
        assert_eq!(payload["import"]["final_height"], 100);
        assert_eq!(payload["import"]["empty_blocks"], 101);
        assert_eq!(payload["import"]["empty_only_blocks"], 101);
        assert_eq!(payload["import"]["empty_block_import_seconds"], 0.0505);
        assert_eq!(payload["import"]["empty_blocks_per_second"], 2000.0);
        assert_eq!(payload["import"]["transaction_blocks"], 0);
        assert_eq!(payload["import"]["transactions"], 0);
        assert_eq!(payload["import"]["transaction_block_import_seconds"], 0.0);
        assert_eq!(payload["import"]["transaction_blocks_per_second"], 0.0);
        assert_eq!(payload["import"]["throughput_status"], "within-target");
        assert_eq!(
            payload["hot_metrics"]["state_service_mpt_avg_total_us"],
            2000
        );
        assert_eq!(
            payload["hot_metrics"]["state_service_mpt_trie_commit_avg_us"],
            1200
        );
        assert_eq!(payload["hot_metrics"]["native_persist_avg_total_us"], 3000);
        assert_eq!(
            payload["hot_metrics"]["native_persist_tx_hot_stage"],
            "application"
        );
        assert_eq!(
            payload["hot_metrics"]["native_persist_tx_hot_stage_avg_us"],
            1700
        );
        assert_eq!(
            payload["hot_metrics"]["rocksdb_batch_avg_flush_duration_ms"],
            11
        );
        assert_eq!(
            payload["hot_metrics"]["rocksdb_batch_pending_operations"],
            19
        );
    }

    #[test]
    fn fast_sync_report_preserves_transaction_bearing_throughput_proof() {
        let package = test_package(0, 100);
        let import_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([100; 32]),
        };
        let report = FastSyncReport::from_parts(
            &package,
            Path::new("/cache/chain.0.acc.zip"),
            Path::new("/cache/chain.0.acc/chain.0.acc"),
            import_report_with_composition(101, Some(import_tip), 0.25, 404.0, 81, 20, 45),
            None,
        );

        assert_eq!(report.import.imported_blocks, 101);
        assert_eq!(report.import.empty_blocks, 81);
        assert_eq!(report.import.empty_only_blocks, 0);
        assert_eq!(report.import.empty_block_import_seconds, 0.0);
        assert_eq!(report.import.empty_blocks_per_second, 0.0);
        assert_eq!(report.import.transaction_blocks, 20);
        assert_eq!(report.import.transactions, 45);
        assert_eq!(report.import.transaction_block_import_seconds, 0.25);
        assert_eq!(report.import.transaction_blocks_per_second, 80.0);
    }

    #[test]
    fn fast_sync_report_serializes_reference_verification_provenance() {
        let temp = tempfile::tempdir().expect("temp");
        let package = test_package(0, 100);
        let import_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([100; 32]),
        };
        let report = FastSyncReport::from_parts(
            &package,
            &temp.path().join("chain.0.acc.zip"),
            &temp.path().join("chain.0.acc/chain.0.acc"),
            import_report(101, Some(import_tip), 0.0505, 2000.0),
            Some(FastSyncReferenceReport {
                endpoint: "https://seed1.neo.org:10332".to_string(),
                block_height: 100,
                block_hash: import_tip.hash.to_string(),
                state_root_height: Some(100),
                state_root_hash: Some(neo_primitives::UInt256::from([7; 32]).to_string()),
            }),
        );
        let path = temp.path().join("proof.json");

        write_fast_sync_report_sidecar(&path, &report).expect("write sidecar");

        let payload: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read sidecar")).expect("json");
        assert_eq!(
            payload["reference"]["endpoint"],
            "https://seed1.neo.org:10332"
        );
        assert_eq!(payload["reference"]["block_height"], 100);
        assert_eq!(
            payload["reference"]["block_hash"],
            import_tip.hash.to_string()
        );
        assert_eq!(payload["reference"]["state_root_height"], 100);
        assert_eq!(
            payload["reference"]["state_root_hash"],
            neo_primitives::UInt256::from([7; 32]).to_string()
        );
    }

    #[test]
    fn fast_sync_post_import_tip_proof_accepts_matching_durable_tip() {
        let store = memory_store_with_ledger_tip(100);
        let package = test_package(0, 100);
        let report = import_report(
            101,
            chain_acc::local_ledger_tip(Some(&store)).expect("read tip"),
            1.0,
            101.0,
        );

        verify_fast_sync_import_tip(&store, &package, &report).expect("matching tip");
    }

    #[test]
    fn fast_sync_post_import_tip_proof_rejects_mismatched_durable_tip() {
        let store = memory_store_with_ledger_tip(99);
        let package = test_package(0, 100);
        let imported_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([100; 32]),
        };
        let report = import_report(101, Some(imported_tip), 1.0, 101.0);

        let err = verify_fast_sync_import_tip(&store, &package, &report)
            .expect_err("mismatched durable tip must fail");

        assert!(
            err.to_string()
                .contains("fast-sync local ledger tip mismatch"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("expected imported tip height 100"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("local durable tip height 99"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fast_sync_post_import_state_root_proof_accepts_matching_local_root() {
        let (state_store, root_hash) = state_store_with_local_root(100);
        let imported_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([0xAB; 32]),
        };

        let proof = local_state_root_tip(Some(&state_store), &test_package(0, 100), imported_tip)
            .expect("local state root proof")
            .expect("state root enabled");

        assert_eq!(proof.index, 100);
        assert_eq!(proof.root_hash, root_hash);
    }

    #[test]
    fn fast_sync_post_import_state_root_proof_rejects_missing_local_root() {
        let state_store = Arc::new(neo_state_service::StateStore::with_mpt(true));
        let imported_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([0xAB; 32]),
        };

        let err = local_state_root_tip(Some(&state_store), &test_package(0, 100), imported_tip)
            .expect_err("missing local state root must fail");

        assert!(
            err.to_string()
                .contains("StateService has no local state root"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fast_sync_post_import_state_root_proof_rejects_stale_local_root() {
        let (state_store, _root_hash) = state_store_with_local_root(99);
        let imported_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([0xAB; 32]),
        };

        let err = local_state_root_tip(Some(&state_store), &test_package(0, 100), imported_tip)
            .expect_err("stale local state root must fail");

        assert!(
            err.to_string().contains(
                "local state-root tip height 99 does not match imported block tip height 100"
            ),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn fast_sync_reference_block_tip_proof_accepts_matching_upstream_hash() {
        let imported_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([0xAB; 32]),
        };
        let endpoint = serve_rpc_once(
            "getblockhash",
            serde_json::json!(imported_tip.hash.to_string()),
        );

        reference::verify_block_tip(&endpoint, &test_package(0, 100), imported_tip)
            .await
            .expect("matching upstream block hash");
    }

    #[tokio::test]
    async fn fast_sync_reference_block_tip_proof_rejects_mismatched_upstream_hash() {
        let imported_tip = chain_acc::LocalLedgerTip {
            height: 100,
            hash: neo_primitives::UInt256::from([0xAB; 32]),
        };
        let upstream_hash = neo_primitives::UInt256::from([0xCD; 32]);
        let endpoint = serve_rpc_once("getblockhash", serde_json::json!(upstream_hash.to_string()));

        let err = reference::verify_block_tip(&endpoint, &test_package(0, 100), imported_tip)
            .await
            .expect_err("mismatched upstream block hash must fail");

        assert!(
            err.to_string()
                .contains("fast-sync reference block hash mismatch"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains("height 100"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains(&imported_tip.hash.to_string()),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains(&upstream_hash.to_string()),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn fast_sync_reference_state_root_proof_accepts_matching_upstream_root() {
        let root_hash = neo_primitives::UInt256::from([0x44; 32]);
        let local_root = LocalStateRootTip {
            index: 100,
            root_hash,
        };
        let endpoint = serve_rpc_once(
            "getstateroot",
            serde_json::json!({
                "version": 0,
                "index": 100,
                "roothash": root_hash.to_string(),
            }),
        );

        reference::verify_state_root_tip(&endpoint, &test_package(0, 100), local_root)
            .await
            .expect("matching upstream state root");
    }

    #[tokio::test]
    async fn fast_sync_reference_state_root_proof_rejects_mismatched_upstream_root() {
        let local_root = LocalStateRootTip {
            index: 100,
            root_hash: neo_primitives::UInt256::from([0x44; 32]),
        };
        let upstream_root = neo_primitives::UInt256::from([0x55; 32]);
        let endpoint = serve_rpc_once(
            "getstateroot",
            serde_json::json!({
                "version": 0,
                "index": 100,
                "roothash": upstream_root.to_string(),
            }),
        );

        let err = reference::verify_state_root_tip(&endpoint, &test_package(0, 100), local_root)
            .await
            .expect_err("mismatched upstream state root must fail");

        assert!(
            err.to_string()
                .contains("fast-sync reference state root mismatch"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains(&local_root.root_hash.to_string()),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string().contains(&upstream_root.to_string()),
            "unexpected error: {err}"
        );
    }
}
