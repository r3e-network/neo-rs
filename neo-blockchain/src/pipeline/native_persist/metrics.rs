//! Low-overhead timing helpers for native block persistence.

pub(crate) fn record_tx_stage(
    stage: neo_runtime::sync_metrics::NativePersistTxStage,
    start: std::time::Instant,
) {
    neo_runtime::sync_metrics::record_native_persist_tx_stage(
        stage,
        neo_runtime::time::elapsed_us(start.elapsed()),
    );
}
