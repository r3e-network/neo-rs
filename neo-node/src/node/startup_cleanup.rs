//! Startup import cleanup and durable-store finalization.
//!
//! Bulk imports temporarily enable backend-specific fast-sync mode for higher
//! write throughput. These helpers keep the failure and shutdown paths in one
//! place so chain and service stores are restored or rolled back consistently
//! before the daemon either continues to live sync or exits.

use std::sync::Arc;

use neo_storage::persistence::{Store, TransactionalStore};

use super::observability;
use super::services::NodeServiceHandles;

pub(super) fn flush_state_service_for_shutdown<S>(
    services: &NodeServiceHandles<S>,
) -> anyhow::Result<()>
where
    S: Store + 'static,
{
    if let Some(state_service) = services.state_commit_handlers() {
        flush_state_service(&state_service)?;
    }
    Ok(())
}

fn flush_state_service<S: Store>(
    state_service: &neo_state_service::commit_handlers::StateServiceCommitHandlers<S>,
) -> anyhow::Result<()> {
    state_service
        .flush_result()
        .map_err(|err| anyhow::anyhow!("state service MPT worker failed during flush: {err}"))
}

pub(super) fn restore_durable_store_mode<S, ServiceS>(
    chain_store: &S,
    service_stores: &[Arc<ServiceS>],
) -> anyhow::Result<()>
where
    S: neo_storage::persistence::store::Store,
    ServiceS: neo_storage::persistence::store::Store,
{
    chain_store.disable_fast_sync_mode();
    chain_store
        .flush()
        .map_err(|err| anyhow::anyhow!("flushing chain store after fast-sync mode: {err}"))?;
    for store in service_stores {
        store.disable_fast_sync_mode();
        store
            .flush()
            .map_err(|err| anyhow::anyhow!("flushing service store after fast-sync mode: {err}"))?;
    }
    Ok(())
}

pub(super) fn abort_fast_sync_store_mode<S, ServiceS>(
    chain_store: &S,
    service_stores: &[Arc<ServiceS>],
) where
    S: neo_storage::persistence::store::Store,
    ServiceS: neo_storage::persistence::store::Store,
{
    chain_store.discard_pending_fast_sync_writes();
    chain_store.disable_fast_sync_mode();
    for store in service_stores {
        store.discard_pending_fast_sync_writes();
        store.disable_fast_sync_mode();
    }
}

pub(super) fn abort_startup_after_import_failure<S, ServiceS>(
    node: &neo_system::Node<neo_native_contracts::StandardNativeProvider, S>,
    services: &NodeServiceHandles<S>,
    durable_service_stores: &[Arc<ServiceS>],
    handles: Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&observability::ObservabilityRuntime>,
    operation: &'static str,
    err: anyhow::Error,
) -> anyhow::Error
where
    S: TransactionalStore + 'static,
    ServiceS: Store + 'static,
{
    let mut cleanup_errors = Vec::new();
    if let Err(cleanup_err) = flush_state_service_for_shutdown(services) {
        cleanup_errors.push(format!("state-service flush failed: {cleanup_err:#}"));
    }
    abort_fast_sync_store_mode(node.storage().as_ref(), durable_service_stores);
    for handle in handles {
        handle.abort();
    }

    let mut message = format!(
        "{operation} failed; startup aborted to avoid continuing with a partial local ledger"
    );
    if !cleanup_errors.is_empty() {
        message.push_str("; cleanup errors: ");
        message.push_str(&cleanup_errors.join("; "));
    }
    let failure = err.context(message);
    if let Some(observability) = observability {
        observability.report_startup_error(&failure);
    }
    failure
}
