//! Startup import cleanup and durable-store finalization.
//!
//! Bulk imports temporarily enable backend-specific fast-sync mode for higher
//! write throughput. These helpers keep the failure and shutdown paths in one
//! place so chain and service stores are restored or rolled back consistently
//! before the daemon either continues to live sync or exits.

use std::sync::Arc;

use super::observability;

pub(super) fn flush_state_service_for_shutdown(node: &neo_system::Node) -> anyhow::Result<()> {
    if let Some(state_service) =
        node.get_service::<neo_state_service::commit_handlers::StateServiceCommitHandlers>()
    {
        flush_state_service(&state_service)?;
    }
    Ok(())
}

fn flush_state_service(
    state_service: &neo_state_service::commit_handlers::StateServiceCommitHandlers,
) -> anyhow::Result<()> {
    state_service
        .flush_result()
        .map_err(|err| anyhow::anyhow!("state service MPT worker failed during flush: {err}"))
}

pub(super) fn restore_durable_store_mode(
    chain_store: &dyn neo_storage::persistence::store::Store,
    service_stores: &[Arc<dyn neo_storage::persistence::store::Store>],
) -> anyhow::Result<()> {
    if let Some(fs) = chain_store.as_fast_sync_store() {
        fs.disable_fast_sync_mode();
    }
    chain_store
        .flush()
        .map_err(|err| anyhow::anyhow!("flushing chain store after fast-sync mode: {err}"))?;
    for store in service_stores {
        if let Some(fs) = store.as_fast_sync_store() {
            fs.disable_fast_sync_mode();
        }
        store
            .flush()
            .map_err(|err| anyhow::anyhow!("flushing service store after fast-sync mode: {err}"))?;
    }
    Ok(())
}

pub(super) fn abort_fast_sync_store_mode(
    chain_store: &dyn neo_storage::persistence::store::Store,
    service_stores: &[Arc<dyn neo_storage::persistence::store::Store>],
) {
    if let Some(fs) = chain_store.as_fast_sync_store() {
        fs.discard_pending_fast_sync_writes();
        fs.disable_fast_sync_mode();
    }
    for store in service_stores {
        if let Some(fs) = store.as_fast_sync_store() {
            fs.discard_pending_fast_sync_writes();
            fs.disable_fast_sync_mode();
        }
    }
}

pub(super) fn abort_startup_after_import_failure(
    node: &neo_system::Node,
    durable_service_stores: &[Arc<dyn neo_storage::persistence::store::Store>],
    handles: Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&observability::ObservabilityRuntime>,
    operation: &'static str,
    err: anyhow::Error,
) -> anyhow::Error {
    let mut cleanup_errors = Vec::new();
    if let Err(cleanup_err) = flush_state_service_for_shutdown(node) {
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
