//! Startup bulk-import orchestration.
//!
//! Chain.acc and fast-sync package imports are startup modes that run before
//! live P2P sync. Their import/parsing mechanics live in lower modules; this
//! module owns the daemon-level decisions around stop-height handling, durable
//! store flushing, task abortion, and observability reporting.

use std::sync::Arc;

use anyhow::Context;
use neo_storage::persistence::{Store, TransactionalStore};
use tracing::{info, warn};

use super::chain_acc;
use super::cli::{NodeCli, import_tip_reaches_stop_height};
use super::config::NodeConfig;
use super::fast_sync;
use super::observability::ObservabilityRuntime;
use super::services::NodeServiceHandles;
use super::startup_cleanup::{
    abort_startup_after_import_failure, flush_durable_stores, flush_state_service_for_shutdown,
};

/// Outcome of startup import processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::node) enum StartupImportOutcome {
    /// Startup imports completed and the daemon should continue to live sync.
    Continue,
    /// A startup import reached `--stop-at-height`; shutdown cleanup is already done.
    StopHeightReached,
}

/// Shared inputs for startup import orchestration.
pub(in crate::node) struct StartupImportContext<'a, S, ServiceS>
where
    S: TransactionalStore + 'static,
    ServiceS: Store + 'static,
{
    pub(in crate::node) cli: &'a NodeCli,
    pub(in crate::node) node:
        &'a Arc<neo_system::Node<neo_native_contracts::StandardNativeProvider, S>>,
    pub(in crate::node) services: &'a NodeServiceHandles<S>,
    pub(in crate::node) config: &'a NodeConfig,
    pub(in crate::node) network: u32,
    pub(in crate::node) durable_service_stores: &'a [Arc<ServiceS>],
    pub(in crate::node) handles: &'a mut Vec<tokio::task::JoinHandle<()>>,
    pub(in crate::node) observability: Option<&'a ObservabilityRuntime>,
}

/// Runs configured startup imports in daemon order: explicit chain.acc first,
/// then official fast-sync package import.
pub(in crate::node) async fn run_startup_imports<S, ServiceS>(
    mut ctx: StartupImportContext<'_, S, ServiceS>,
) -> anyhow::Result<StartupImportOutcome>
where
    S: TransactionalStore + 'static,
    ServiceS: Store + 'static,
{
    if run_chain_acc_import(&mut ctx).await? == StartupImportOutcome::StopHeightReached {
        return Ok(StartupImportOutcome::StopHeightReached);
    }
    run_fast_sync_import(&mut ctx).await
}

async fn run_chain_acc_import<S, ServiceS>(
    ctx: &mut StartupImportContext<'_, S, ServiceS>,
) -> anyhow::Result<StartupImportOutcome>
where
    S: TransactionalStore + 'static,
    ServiceS: Store + 'static,
{
    let Some(import_path) = &ctx.cli.import_chain else {
        return Ok(StartupImportOutcome::Continue);
    };

    let blockchain = ctx.node.blockchain();
    match chain_acc::import_chain_acc_until_height(
        &blockchain,
        import_path,
        false,
        ctx.cli.stop_at_height,
        Some(ctx.node.storage()),
    )
    .await
    {
        Ok(count) => {
            flush_durable_stores(ctx.node.storage().as_ref(), ctx.durable_service_stores)?;
            info!(
                target: "neo",
                imported = count,
                "chain.acc import completed successfully; continuing with network sync"
            );
            match blockchain.get_height().await {
                Ok(height) if import_tip_reaches_stop_height(height, ctx.cli.stop_at_height) => {
                    info!(
                        target: "neo",
                        height,
                        stop_at_height = ctx.cli.stop_at_height.unwrap_or_default(),
                        imported = count,
                        "chain.acc import reached stop height; shutting down"
                    );
                    finish_stop_height_import(ctx)?;
                    Ok(StartupImportOutcome::StopHeightReached)
                }
                Ok(_) => Ok(StartupImportOutcome::Continue),
                Err(err) => {
                    warn!(
                        target: "neo",
                        error = %err,
                        "failed to read chain height after chain.acc import; continuing with network sync"
                    );
                    Ok(StartupImportOutcome::Continue)
                }
            }
        }
        Err(err) => Err(abort_startup_after_import_failure(
            ctx.node,
            ctx.services,
            ctx.durable_service_stores,
            std::mem::take(ctx.handles),
            ctx.observability,
            "chain.acc import",
            err,
        )),
    }
}

async fn run_fast_sync_import<S, ServiceS>(
    ctx: &mut StartupImportContext<'_, S, ServiceS>,
) -> anyhow::Result<StartupImportOutcome>
where
    S: TransactionalStore + 'static,
    ServiceS: Store + 'static,
{
    if !ctx.cli.fast_sync {
        return Ok(StartupImportOutcome::Continue);
    }

    let blockchain = ctx.node.blockchain();
    let state_store = ctx.services.state_store();
    let state_service = ctx.services.state_commit_handlers();
    match fast_sync::run_fast_sync_report(
        &blockchain,
        ctx.node.storage(),
        ctx.config,
        ctx.cli.storage_path.as_deref(),
        ctx.cli.fast_sync_cache.as_deref(),
        ctx.network,
        ctx.cli.stop_at_height,
        ctx.cli.fast_sync_reference_rpc.as_deref(),
        ctx.cli.fast_sync_expected_sha256.as_deref(),
        state_store.as_ref(),
        state_service.as_ref(),
    )
    .await
    {
        Ok(report) => {
            if let Some(path) = &ctx.cli.fast_sync_report {
                fast_sync::write_fast_sync_report_sidecar(path, &report)?;
            }
            flush_durable_stores(ctx.node.storage().as_ref(), ctx.durable_service_stores)?;
            let count = report.import.imported_blocks;
            info!(
                target: "neo::fast_sync",
                imported = count,
                package = %report.package.filename,
                end_height = report.package.end_height,
                average_blocks_per_second = report.import.average_blocks_per_second,
                throughput_status = ?report.import.throughput_status,
                "fast-sync package import completed successfully; continuing with network sync"
            );
            match blockchain.get_height().await {
                Ok(height) if import_tip_reaches_stop_height(height, ctx.cli.stop_at_height) => {
                    info!(
                        target: "neo::fast_sync",
                        height,
                        stop_at_height = ctx.cli.stop_at_height.unwrap_or_default(),
                        imported = count,
                        "fast-sync import reached stop height; shutting down"
                    );
                    finish_stop_height_import(ctx)?;
                    Ok(StartupImportOutcome::StopHeightReached)
                }
                Ok(_) => Ok(StartupImportOutcome::Continue),
                Err(err) => {
                    warn!(
                        target: "neo::fast_sync",
                        error = %err,
                        "failed to read chain height after fast-sync import; continuing with network sync"
                    );
                    Ok(StartupImportOutcome::Continue)
                }
            }
        }
        Err(err) => Err(abort_startup_after_import_failure(
            ctx.node,
            ctx.services,
            ctx.durable_service_stores,
            std::mem::take(ctx.handles),
            ctx.observability,
            "fast-sync package import",
            err,
        )),
    }
}

fn finish_stop_height_import<S, ServiceS>(
    ctx: &mut StartupImportContext<'_, S, ServiceS>,
) -> anyhow::Result<()>
where
    S: TransactionalStore + 'static,
    ServiceS: Store + 'static,
{
    flush_state_service_for_shutdown(ctx.services)?;
    flush_durable_stores(ctx.node.storage().as_ref(), ctx.durable_service_stores)
        .context("failed to flush durable stores after stop-height startup import")?;
    for handle in ctx.handles.drain(..) {
        handle.abort();
    }
    Ok(())
}
