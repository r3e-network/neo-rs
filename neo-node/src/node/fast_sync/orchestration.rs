//! Fast-sync package import orchestration.
//!
//! This module coordinates the focused fast-sync components: package cache and
//! extraction, local proof checks, crash-safety marker handling, optional
//! reference RPC proof, and final machine-readable reporting.

use std::path::Path;
use std::sync::Arc;

use neo_blockchain::BlockchainHandle;
use neo_state_service::StateStore;
use neo_state_service::commit_handlers::StateServiceCommitHandlers;
use neo_storage::persistence::store::Store;
use tracing::info;

use super::super::config::NodeConfig;
use super::cache_dir::fast_sync_cache_dir;
use super::local::{
    local_state_root_tip, validate_fast_sync_preflight, verify_fast_sync_import_tip,
};
use super::marker::{
    clear_fast_sync_import_marker, refuse_stale_fast_sync_import_marker,
    write_fast_sync_import_marker,
};
use super::package::{ensure_chain_acc_extracted, ensure_package_cached, fetch_latest_package};
use super::reference;
use super::report::{FastSyncReferenceReport, FastSyncReport, log_fast_sync_throughput};

pub(in crate::node) async fn run_fast_sync_report(
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
    let report = super::super::chain_acc::import_chain_acc_report_with_expected_range(
        blockchain,
        &chain_path,
        false,
        super::super::chain_acc::ChainAccExpectedRange {
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
