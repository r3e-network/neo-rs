//! # neo-node::node::sync_metrics
//!
//! Sync-speed counters, summaries, and operator-facing throughput status.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `sync_metrics`: Prometheus rendering for node sync metrics.

mod families;
mod writer;

use families::{
    append_native_contract_hooks, append_native_persist_tx_stages,
    append_neo_token_candidate_counts, append_neo_token_committee_compute_stages,
    append_neo_token_onpersist_stages, append_state_root_apply_metrics,
};

/// Render sync metrics as Prometheus-format text.
pub fn render_prometheus() -> String {
    use neo_runtime::sync_metrics as m;

    let height = m::height();
    let peer_tip = m::peer_live_tip();
    let blocks = m::blocks_persisted();
    let state_apply = neo_state_service::StateRootApplyMetrics::state_root_apply_stats();

    let mut output = format!(
        "# HELP neo_sync_height Current block height\n\
         # TYPE neo_sync_height gauge\n\
         neo_sync_height {height}\n\
         # HELP neo_sync_peer_tip Peer-reported live chain tip\n\
         # TYPE neo_sync_peer_tip gauge\n\
         neo_sync_peer_tip {peer_tip}\n\
         # HELP neo_sync_lag Blocks behind live tip\n\
         # TYPE neo_sync_lag gauge\n\
         neo_sync_lag {}\n\
         # HELP neo_sync_blocks_persisted Total blocks persisted since startup\n\
         # TYPE neo_sync_blocks_persisted counter\n\
         neo_sync_blocks_persisted {blocks}\n\
         # HELP neo_sync_avg_total_us EWMA total per-block persist time (microseconds)\n\
         # TYPE neo_sync_avg_total_us gauge\n\
         neo_sync_avg_total_us {}\n\
         # HELP neo_sync_avg_verify_us EWMA witness verification time (microseconds)\n\
         # TYPE neo_sync_avg_verify_us gauge\n\
         neo_sync_avg_verify_us {}\n\
         # HELP neo_sync_avg_persist_us EWMA native contract execution time (microseconds)\n\
         # TYPE neo_sync_avg_persist_us gauge\n\
         neo_sync_avg_persist_us {}\n\
         # HELP neo_sync_avg_commit_us EWMA persistent-store commit time (microseconds)\n\
         # TYPE neo_sync_avg_commit_us gauge\n\
         neo_sync_avg_commit_us {}\n\
         # HELP neo_sync_native_persist_blocks_total Total native persistence records since startup\n\
         # TYPE neo_sync_native_persist_blocks_total counter\n\
         neo_sync_native_persist_blocks_total {}\n\
         # HELP neo_sync_native_persist_height Latest block height observed by native persistence metrics\n\
         # TYPE neo_sync_native_persist_height gauge\n\
         neo_sync_native_persist_height {}\n\
         # HELP neo_sync_native_persist_avg_total_us EWMA total native persistence time (microseconds)\n\
         # TYPE neo_sync_native_persist_avg_total_us gauge\n\
         neo_sync_native_persist_avg_total_us {}\n\
         # HELP neo_sync_native_persist_avg_onpersist_us EWMA native OnPersist stage time (microseconds)\n\
         # TYPE neo_sync_native_persist_avg_onpersist_us gauge\n\
         neo_sync_native_persist_avg_onpersist_us {}\n\
         # HELP neo_sync_native_persist_avg_tx_us EWMA per-transaction Application stage time (microseconds)\n\
         # TYPE neo_sync_native_persist_avg_tx_us gauge\n\
         neo_sync_native_persist_avg_tx_us {}\n\
         # HELP neo_sync_native_persist_avg_postpersist_us EWMA native PostPersist stage time (microseconds)\n\
         # TYPE neo_sync_native_persist_avg_postpersist_us gauge\n\
         neo_sync_native_persist_avg_postpersist_us {}\n\
         # HELP neo_sync_native_persist_avg_cache_commit_us EWMA native persistence staged cache merge time (microseconds)\n\
         # TYPE neo_sync_native_persist_avg_cache_commit_us gauge\n\
         neo_sync_native_persist_avg_cache_commit_us {}\n\
         # HELP neo_sync_native_persist_avg_tx_count EWMA transaction count per native persistence call\n\
         # TYPE neo_sync_native_persist_avg_tx_count gauge\n\
         neo_sync_native_persist_avg_tx_count {}\n\
         # HELP neo_state_service_mpt_apply_blocks_total Total local StateService MPT apply attempts\n\
         # TYPE neo_state_service_mpt_apply_blocks_total counter\n\
         neo_state_service_mpt_apply_blocks_total {}\n\
         # HELP neo_state_service_mpt_apply_failures_total Total failed local StateService MPT apply attempts\n\
         # TYPE neo_state_service_mpt_apply_failures_total counter\n\
         neo_state_service_mpt_apply_failures_total {}\n\
         # HELP neo_state_service_mpt_apply_height Latest block height observed by local StateService MPT apply\n\
         # TYPE neo_state_service_mpt_apply_height gauge\n\
         neo_state_service_mpt_apply_height {}\n\
         # HELP neo_state_service_mpt_apply_avg_total_us EWMA total local StateService MPT apply time (microseconds)\n\
         # TYPE neo_state_service_mpt_apply_avg_total_us gauge\n\
         neo_state_service_mpt_apply_avg_total_us {}\n\
         # HELP neo_state_service_mpt_apply_avg_project_us EWMA DataCache-to-MPT changeset projection time (microseconds)\n\
         # TYPE neo_state_service_mpt_apply_avg_project_us gauge\n\
         neo_state_service_mpt_apply_avg_project_us {}\n\
         # HELP neo_state_service_mpt_apply_avg_trie_us EWMA trie/write local StateService MPT apply time (microseconds)\n\
         # TYPE neo_state_service_mpt_apply_avg_trie_us gauge\n\
         neo_state_service_mpt_apply_avg_trie_us {}\n\
         # HELP neo_state_service_mpt_apply_avg_changes EWMA projected StateService MPT changes per block\n\
         # TYPE neo_state_service_mpt_apply_avg_changes gauge\n\
         neo_state_service_mpt_apply_avg_changes {}\n",
        peer_tip.saturating_sub(height),
        m::avg_total_us(),
        m::avg_verify_us(),
        m::avg_persist_us(),
        m::avg_commit_us(),
        m::native_persist_blocks(),
        m::native_persist_height(),
        m::native_persist_avg_total_us(),
        m::native_persist_avg_onpersist_us(),
        m::native_persist_avg_tx_us(),
        m::native_persist_avg_postpersist_us(),
        m::native_persist_avg_cache_commit_us(),
        m::native_persist_avg_tx_count(),
        state_apply.attempts,
        state_apply.failures,
        state_apply.latest_height,
        state_apply.avg_total_us,
        state_apply.avg_project_us,
        state_apply.avg_apply_us,
        state_apply.avg_changes,
    );

    append_state_root_apply_metrics(&mut output);
    append_native_contract_hooks(&mut output);
    append_native_persist_tx_stages(&mut output);
    append_neo_token_onpersist_stages(&mut output);
    append_neo_token_committee_compute_stages(&mut output);
    append_neo_token_candidate_counts(&mut output);

    output
}

#[cfg(test)]
mod tests {
    use super::render_prometheus;

    #[test]
    fn render_prometheus_keeps_labelled_metric_families() {
        neo_state_service::metrics::StateRootApplyMetrics::record_stage(
            neo_state_service::metrics::StateRootApplyStage::RootHash,
            17,
        );
        neo_state_service::metrics::StateRootApplyMetrics::record_count(
            neo_state_service::metrics::StateRootApplyCountKind::OverlayEntries,
            3,
        );
        neo_runtime::sync_metrics::record_native_contract_hook(
            neo_runtime::sync_metrics::NativePersistHook::OnPersist,
            -5,
            23,
        );
        neo_runtime::sync_metrics::record_native_persist_tx_stage(
            neo_runtime::sync_metrics::NativePersistTxStage::Execute,
            29,
        );
        neo_runtime::sync_metrics::record_neo_token_onpersist_stage(
            neo_runtime::sync_metrics::NeoTokenOnPersistStage::RefreshTotal,
            31,
        );
        neo_runtime::sync_metrics::record_neo_token_committee_compute_stage(
            neo_runtime::sync_metrics::NeoTokenCommitteeComputeStage::TopCandidateMaintenance,
            37,
        );
        neo_runtime::sync_metrics::record_neo_token_committee_candidate_count(
            neo_runtime::sync_metrics::NeoTokenCommitteeCandidateCount::EligibleCandidates,
            5,
        );

        let output = render_prometheus();

        assert!(output.contains("# HELP neo_sync_height Current block height"));
        assert!(
            output.contains("neo_state_service_mpt_apply_stage_calls_total{stage=\"root_hash\"} "),
            "state-root stage labels should stay Prometheus-compatible"
        );
        assert!(
            output.contains(
                "neo_state_service_mpt_apply_count_samples_total{kind=\"overlay_entries\"} "
            ),
            "state-root count labels should stay Prometheus-compatible"
        );
        assert!(
            output.contains(
                "neo_sync_native_contract_hook_calls_total{trigger=\"onpersist\",contract=\"NeoToken\",id=\"-5\"} "
            ),
            "native hook labels should preserve trigger, contract, and id"
        );
        assert!(
            output.contains("neo_sync_native_persist_tx_stage_calls_total{stage=\"execute\"} "),
            "native transaction-stage labels should stay stable"
        );
        assert!(
            output.contains(
                "neo_sync_neotoken_onpersist_stage_calls_total{stage=\"refresh_total\"} "
            ),
            "NeoToken OnPersist stage labels should stay stable"
        );
        assert!(
            output.contains(
                "neo_sync_neotoken_committee_compute_stage_calls_total{stage=\"top_candidate_maintenance\"} "
            ),
            "NeoToken committee-compute stage labels should stay stable"
        );
        assert!(
            output.contains(
                "neo_sync_neotoken_committee_candidate_scan_samples_total{kind=\"eligible_candidates\"} "
            ),
            "NeoToken candidate-scan count labels should stay stable"
        );
    }
}
