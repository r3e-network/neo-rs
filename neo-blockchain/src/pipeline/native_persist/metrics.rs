//! Low-overhead timing helpers for native block persistence.

pub(crate) fn record_tx_stage(
    stage: neo_runtime::sync_metrics::NativePersistTxStage,
    start: std::time::Instant,
) {
    record_tx_stage_elapsed(stage, start.elapsed());
}

pub(crate) fn record_tx_stage_elapsed(
    stage: neo_runtime::sync_metrics::NativePersistTxStage,
    elapsed: std::time::Duration,
) -> u64 {
    let elapsed_us = neo_runtime::time::elapsed_us(elapsed);
    neo_runtime::sync_metrics::record_native_persist_tx_stage(stage, elapsed_us);
    elapsed_us
}
