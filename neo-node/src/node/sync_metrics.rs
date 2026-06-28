//! Performance metrics rendering for the sync + execution pipeline.
//!
//! Reads the lock-free atomics from [`neo_runtime::sync_metrics`] and renders
//! them as Prometheus-format text for the /metrics telemetry endpoint. This is
//! the "analysis system" that continuously monitors sync and execution hot spots.
//!
//! The actual recording happens in [`neo_runtime::sync_metrics::record_block`],
//! called from the blockchain-service block-persist hot path.

/// Render sync metrics as Prometheus-format text.
pub fn render_prometheus() -> String {
    use std::fmt::Write as _;

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
         # HELP neo_sync_avg_commit_us EWMA RocksDB commit time (microseconds)\n\
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

    output.push_str(
        "# HELP neo_state_service_mpt_apply_stage_calls_total Total fine-grained local StateService MPT apply stage observations\n\
         # TYPE neo_state_service_mpt_apply_stage_calls_total counter\n\
         # HELP neo_state_service_mpt_apply_stage_avg_us EWMA fine-grained local StateService MPT apply stage time (microseconds)\n\
         # TYPE neo_state_service_mpt_apply_stage_avg_us gauge\n",
    );
    for stat in neo_state_service::StateRootApplyMetrics::state_root_apply_stage_stats() {
        writeln!(
            output,
            "neo_state_service_mpt_apply_stage_calls_total{{stage=\"{}\"}} {}",
            stat.stage, stat.calls
        )
        .expect("write metrics line");
        writeln!(
            output,
            "neo_state_service_mpt_apply_stage_avg_us{{stage=\"{}\"}} {}",
            stat.stage, stat.avg_us
        )
        .expect("write metrics line");
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
        writeln!(
            output,
            "neo_state_service_mpt_apply_count_samples_total{{kind=\"{}\"}} {}",
            stat.kind, stat.samples
        )
        .expect("write metrics line");
        writeln!(
            output,
            "neo_state_service_mpt_apply_items_total{{kind=\"{}\"}} {}",
            stat.kind, stat.total
        )
        .expect("write metrics line");
        writeln!(
            output,
            "neo_state_service_mpt_apply_avg_items{{kind=\"{}\"}} {}",
            stat.kind, stat.avg
        )
        .expect("write metrics line");
    }

    output.push_str(
        "# HELP neo_sync_native_contract_hook_calls_total Total native contract persist hook calls by trigger and contract\n\
         # TYPE neo_sync_native_contract_hook_calls_total counter\n\
         # HELP neo_sync_native_contract_hook_avg_us EWMA native contract persist hook time by trigger and contract (microseconds)\n\
         # TYPE neo_sync_native_contract_hook_avg_us gauge\n",
    );
    for stat in m::native_contract_hook_stats() {
        writeln!(
            output,
            "neo_sync_native_contract_hook_calls_total{{trigger=\"{}\",contract=\"{}\",id=\"{}\"}} {}",
            stat.trigger, stat.contract, stat.contract_id, stat.calls
        )
        .expect("write metrics line");
        writeln!(
            output,
            "neo_sync_native_contract_hook_avg_us{{trigger=\"{}\",contract=\"{}\",id=\"{}\"}} {}",
            stat.trigger, stat.contract, stat.contract_id, stat.avg_us
        )
        .expect("write metrics line");
    }
    output.push_str(
        "# HELP neo_sync_neotoken_onpersist_stage_calls_total Total NeoToken OnPersist stage observations\n\
         # TYPE neo_sync_neotoken_onpersist_stage_calls_total counter\n\
         # HELP neo_sync_neotoken_onpersist_stage_avg_us EWMA NeoToken OnPersist stage time (microseconds)\n\
         # TYPE neo_sync_neotoken_onpersist_stage_avg_us gauge\n",
    );
    for stat in m::neo_token_onpersist_stage_stats() {
        writeln!(
            output,
            "neo_sync_neotoken_onpersist_stage_calls_total{{stage=\"{}\"}} {}",
            stat.stage, stat.calls
        )
        .expect("write metrics line");
        writeln!(
            output,
            "neo_sync_neotoken_onpersist_stage_avg_us{{stage=\"{}\"}} {}",
            stat.stage, stat.avg_us
        )
        .expect("write metrics line");
    }
    output.push_str(
        "# HELP neo_sync_neotoken_committee_compute_stage_calls_total Total NeoToken committee-compute stage observations\n\
         # TYPE neo_sync_neotoken_committee_compute_stage_calls_total counter\n\
         # HELP neo_sync_neotoken_committee_compute_stage_avg_us EWMA NeoToken committee-compute stage time (microseconds)\n\
         # TYPE neo_sync_neotoken_committee_compute_stage_avg_us gauge\n",
    );
    for stat in m::neo_token_committee_compute_stage_stats() {
        writeln!(
            output,
            "neo_sync_neotoken_committee_compute_stage_calls_total{{stage=\"{}\"}} {}",
            stat.stage, stat.calls
        )
        .expect("write metrics line");
        writeln!(
            output,
            "neo_sync_neotoken_committee_compute_stage_avg_us{{stage=\"{}\"}} {}",
            stat.stage, stat.avg_us
        )
        .expect("write metrics line");
    }
    output.push_str(
        "# HELP neo_sync_neotoken_committee_candidate_scan_samples_total Total NeoToken committee candidate-scan count samples\n\
         # TYPE neo_sync_neotoken_committee_candidate_scan_samples_total counter\n\
         # HELP neo_sync_neotoken_committee_candidate_scan_items_total Cumulative NeoToken committee candidate-scan items\n\
         # TYPE neo_sync_neotoken_committee_candidate_scan_items_total counter\n\
         # HELP neo_sync_neotoken_committee_candidate_scan_avg_items EWMA NeoToken committee candidate-scan items per scan\n\
         # TYPE neo_sync_neotoken_committee_candidate_scan_avg_items gauge\n",
    );
    for stat in m::neo_token_committee_candidate_count_stats() {
        writeln!(
            output,
            "neo_sync_neotoken_committee_candidate_scan_samples_total{{kind=\"{}\"}} {}",
            stat.kind, stat.samples
        )
        .expect("write metrics line");
        writeln!(
            output,
            "neo_sync_neotoken_committee_candidate_scan_items_total{{kind=\"{}\"}} {}",
            stat.kind, stat.total
        )
        .expect("write metrics line");
        writeln!(
            output,
            "neo_sync_neotoken_committee_candidate_scan_avg_items{{kind=\"{}\"}} {}",
            stat.kind, stat.avg
        )
        .expect("write metrics line");
    }

    output
}
