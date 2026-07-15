//! Metric-family renderers for sync-related Prometheus output.
//!
//! Keep each function aligned with one bounded-label metric family so the
//! top-level renderer reads like an operator-facing summary instead of a long
//! sequence of formatting mechanics.

use super::writer::{push_native_hook_metric, push_single_label_metric};

pub(super) fn append_mdbx_commit_metrics(output: &mut String) {
    let snapshot = neo_storage::mdbx::MdbxCommitMetrics::snapshot();
    let stats = snapshot.stats;
    output.push_str(
        "# HELP neo_storage_mdbx_commit_attempts_total Total raw-overlay MDBX commit paths entered\n\
         # TYPE neo_storage_mdbx_commit_attempts_total counter\n",
    );
    output.push_str(&format!(
        "neo_storage_mdbx_commit_attempts_total {}\n",
        stats.attempts
    ));
    output.push_str(
        "# HELP neo_storage_mdbx_commit_failures_total Total raw-overlay MDBX commit paths that failed\n\
         # TYPE neo_storage_mdbx_commit_failures_total counter\n",
    );
    output.push_str(&format!(
        "neo_storage_mdbx_commit_failures_total {}\n",
        stats.failures
    ));
    output.push_str(
        "# HELP neo_storage_mdbx_committed_transactions_total Total successfully committed MDBX write transactions\n\
         # TYPE neo_storage_mdbx_committed_transactions_total counter\n",
    );
    output.push_str(&format!(
        "neo_storage_mdbx_committed_transactions_total {}\n",
        stats.committed_transactions
    ));
    output.push_str(
        "# HELP neo_storage_mdbx_commit_stage_calls_total Total MDBX commit-stage observations\n\
         # TYPE neo_storage_mdbx_commit_stage_calls_total counter\n\
         # HELP neo_storage_mdbx_commit_stage_duration_us_total Cumulative MDBX commit-stage time (microseconds)\n\
         # TYPE neo_storage_mdbx_commit_stage_duration_us_total counter\n\
         # HELP neo_storage_mdbx_commit_stage_avg_us Average MDBX commit-stage time (microseconds)\n\
         # TYPE neo_storage_mdbx_commit_stage_avg_us gauge\n",
    );
    for stat in snapshot.stages {
        push_single_label_metric(
            output,
            "neo_storage_mdbx_commit_stage_calls_total",
            "stage",
            stat.stage,
            stat.calls,
        );
        push_single_label_metric(
            output,
            "neo_storage_mdbx_commit_stage_duration_us_total",
            "stage",
            stat.stage,
            stat.total_us,
        );
        push_single_label_metric(
            output,
            "neo_storage_mdbx_commit_stage_avg_us",
            "stage",
            stat.stage,
            stat.avg_us,
        );
    }
    output.push_str(
        "# HELP neo_storage_mdbx_commit_volume_samples_total Total MDBX commit volume samples\n\
         # TYPE neo_storage_mdbx_commit_volume_samples_total counter\n\
         # HELP neo_storage_mdbx_commit_volume_total Cumulative MDBX commit entries or bytes\n\
         # TYPE neo_storage_mdbx_commit_volume_total counter\n\
         # HELP neo_storage_mdbx_commit_avg_volume Average MDBX commit entries or bytes per attempt\n\
         # TYPE neo_storage_mdbx_commit_avg_volume gauge\n",
    );
    for stat in snapshot.counts {
        push_single_label_metric(
            output,
            "neo_storage_mdbx_commit_volume_samples_total",
            "kind",
            stat.kind,
            stat.samples,
        );
        push_single_label_metric(
            output,
            "neo_storage_mdbx_commit_volume_total",
            "kind",
            stat.kind,
            stat.total,
        );
        push_single_label_metric(
            output,
            "neo_storage_mdbx_commit_avg_volume",
            "kind",
            stat.kind,
            stat.avg,
        );
    }
}

pub(super) fn append_state_root_apply_metrics(output: &mut String) {
    output.push_str(
        "# HELP neo_state_service_mpt_apply_stage_calls_total Total fine-grained local StateService MPT apply stage observations\n\
         # TYPE neo_state_service_mpt_apply_stage_calls_total counter\n\
         # HELP neo_state_service_mpt_apply_stage_duration_us_total Cumulative fine-grained local StateService MPT apply stage time (microseconds)\n\
         # TYPE neo_state_service_mpt_apply_stage_duration_us_total counter\n\
         # HELP neo_state_service_mpt_apply_stage_avg_us EWMA fine-grained local StateService MPT apply stage time (microseconds)\n\
         # TYPE neo_state_service_mpt_apply_stage_avg_us gauge\n",
    );
    for stat in neo_state_service::StateRootApplyMetrics::state_root_apply_stage_stats() {
        push_single_label_metric(
            output,
            "neo_state_service_mpt_apply_stage_calls_total",
            "stage",
            stat.stage,
            stat.calls,
        );
        push_single_label_metric(
            output,
            "neo_state_service_mpt_apply_stage_duration_us_total",
            "stage",
            stat.stage,
            stat.total_us,
        );
        push_single_label_metric(
            output,
            "neo_state_service_mpt_apply_stage_avg_us",
            "stage",
            stat.stage,
            stat.avg_us,
        );
    }
    output.push_str(
        "# HELP neo_state_service_mpt_apply_count_samples_total Total local StateService MPT apply count samples\n\
         # TYPE neo_state_service_mpt_apply_count_samples_total counter\n\
         # HELP neo_state_service_mpt_apply_items_total Cumulative local StateService MPT apply items\n\
         # TYPE neo_state_service_mpt_apply_items_total counter\n\
         # HELP neo_state_service_mpt_apply_avg_items EWMA local StateService MPT apply items per block\n\
         # TYPE neo_state_service_mpt_apply_avg_items gauge\n",
    );
    for stat in neo_state_service::StateRootApplyMetrics::state_root_apply_count_stats() {
        push_single_label_metric(
            output,
            "neo_state_service_mpt_apply_count_samples_total",
            "kind",
            stat.kind,
            stat.samples,
        );
        push_single_label_metric(
            output,
            "neo_state_service_mpt_apply_items_total",
            "kind",
            stat.kind,
            stat.total,
        );
        push_single_label_metric(
            output,
            "neo_state_service_mpt_apply_avg_items",
            "kind",
            stat.kind,
            stat.avg,
        );
    }
}

pub(super) fn append_native_contract_hooks(output: &mut String) {
    use neo_runtime::sync_metrics as m;

    output.push_str(
        "# HELP neo_sync_native_contract_hook_calls_total Total native contract persist hook calls by trigger and contract\n\
         # TYPE neo_sync_native_contract_hook_calls_total counter\n\
         # HELP neo_sync_native_contract_hook_avg_us EWMA native contract persist hook time by trigger and contract (microseconds)\n\
         # TYPE neo_sync_native_contract_hook_avg_us gauge\n",
    );
    for stat in m::native_contract_hook_stats() {
        push_native_hook_metric(
            output,
            "neo_sync_native_contract_hook_calls_total",
            stat.trigger,
            stat.contract,
            stat.contract_id,
            stat.calls,
        );
        push_native_hook_metric(
            output,
            "neo_sync_native_contract_hook_avg_us",
            stat.trigger,
            stat.contract,
            stat.contract_id,
            stat.avg_us,
        );
    }
}

pub(super) fn append_native_persist_tx_stages(output: &mut String) {
    use neo_runtime::sync_metrics as m;

    output.push_str(
        "# HELP neo_sync_native_persist_tx_stage_calls_total Total native persistence transaction-stage observations\n\
         # TYPE neo_sync_native_persist_tx_stage_calls_total counter\n\
         # HELP neo_sync_native_persist_tx_stage_avg_us EWMA native persistence transaction-stage time (microseconds)\n\
         # TYPE neo_sync_native_persist_tx_stage_avg_us gauge\n",
    );
    for stat in m::native_persist_tx_stage_stats() {
        push_single_label_metric(
            output,
            "neo_sync_native_persist_tx_stage_calls_total",
            "stage",
            stat.stage,
            stat.calls,
        );
        push_single_label_metric(
            output,
            "neo_sync_native_persist_tx_stage_avg_us",
            "stage",
            stat.stage,
            stat.avg_us,
        );
    }
}

pub(super) fn append_neo_token_onpersist_stages(output: &mut String) {
    use neo_runtime::sync_metrics as m;

    output.push_str(
        "# HELP neo_sync_neotoken_onpersist_stage_calls_total Total NeoToken OnPersist stage observations\n\
         # TYPE neo_sync_neotoken_onpersist_stage_calls_total counter\n\
         # HELP neo_sync_neotoken_onpersist_stage_avg_us EWMA NeoToken OnPersist stage time (microseconds)\n\
         # TYPE neo_sync_neotoken_onpersist_stage_avg_us gauge\n",
    );
    for stat in m::neo_token_onpersist_stage_stats() {
        push_single_label_metric(
            output,
            "neo_sync_neotoken_onpersist_stage_calls_total",
            "stage",
            stat.stage,
            stat.calls,
        );
        push_single_label_metric(
            output,
            "neo_sync_neotoken_onpersist_stage_avg_us",
            "stage",
            stat.stage,
            stat.avg_us,
        );
    }
}

pub(super) fn append_neo_token_committee_compute_stages(output: &mut String) {
    use neo_runtime::sync_metrics as m;

    output.push_str(
        "# HELP neo_sync_neotoken_committee_compute_stage_calls_total Total NeoToken committee-compute stage observations\n\
         # TYPE neo_sync_neotoken_committee_compute_stage_calls_total counter\n\
         # HELP neo_sync_neotoken_committee_compute_stage_avg_us EWMA NeoToken committee-compute stage time (microseconds)\n\
         # TYPE neo_sync_neotoken_committee_compute_stage_avg_us gauge\n",
    );
    for stat in m::neo_token_committee_compute_stage_stats() {
        push_single_label_metric(
            output,
            "neo_sync_neotoken_committee_compute_stage_calls_total",
            "stage",
            stat.stage,
            stat.calls,
        );
        push_single_label_metric(
            output,
            "neo_sync_neotoken_committee_compute_stage_avg_us",
            "stage",
            stat.stage,
            stat.avg_us,
        );
    }
}

pub(super) fn append_neo_token_candidate_counts(output: &mut String) {
    use neo_runtime::sync_metrics as m;

    output.push_str(
        "# HELP neo_sync_neotoken_committee_candidate_scan_samples_total Total NeoToken committee candidate-scan count samples\n\
         # TYPE neo_sync_neotoken_committee_candidate_scan_samples_total counter\n\
         # HELP neo_sync_neotoken_committee_candidate_scan_items_total Cumulative NeoToken committee candidate-scan items\n\
         # TYPE neo_sync_neotoken_committee_candidate_scan_items_total counter\n\
         # HELP neo_sync_neotoken_committee_candidate_scan_avg_items EWMA NeoToken committee candidate-scan items per scan\n\
         # TYPE neo_sync_neotoken_committee_candidate_scan_avg_items gauge\n",
    );
    for stat in m::neo_token_committee_candidate_count_stats() {
        push_single_label_metric(
            output,
            "neo_sync_neotoken_committee_candidate_scan_samples_total",
            "kind",
            stat.kind,
            stat.samples,
        );
        push_single_label_metric(
            output,
            "neo_sync_neotoken_committee_candidate_scan_items_total",
            "kind",
            stat.kind,
            stat.total,
        );
        push_single_label_metric(
            output,
            "neo_sync_neotoken_committee_candidate_scan_avg_items",
            "kind",
            stat.kind,
            stat.avg,
        );
    }
}
