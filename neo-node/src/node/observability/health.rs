//! Node readiness and optional service health snapshots for observability hooks.

use serde_json::{Value, json};

use super::super::remote_ledger::RemoteLedgerStatus;
use super::observability_ledger_height;

pub(super) fn node_health_payload(node: &neo_system::Node) -> Value {
    let ledger_height = observability_ledger_height(node);
    let remote_ledger = node.get_service::<RemoteLedgerStatus>();
    let ledger_source = if remote_ledger.is_some() {
        "remote_rpc"
    } else {
        "local"
    };
    let (indexer_payload, indexer_ready) = indexer_health_payload(node, ledger_height);
    let ready = ledger_height.is_some() && indexer_ready;
    let local_info = node.network().local_node_info();
    let mempool = node.mempool();

    json!({
        "status": if ready { "ready" } else { "starting" },
        "ready": ready,
        "ledger_source": ledger_source,
        "remote_ledger_rpc": remote_ledger.as_ref().map(|status| status.endpoint.as_str()),
        "remote_ledger_error": remote_ledger.as_ref().and_then(|status| status.tip_error.as_deref()),
        "ledger_height": ledger_height,
        "connected_peers": local_info.connected_peers_count(),
        "mempool": {
            "transactions": mempool.total_count(),
            "verified": mempool.verified_count(),
            "unverified": mempool.unverified_count(),
        },
        "header_cache_entries": node.header_cache().count(),
        "services": {
            "state_service": {"enabled": node.state_store().is_some()},
            "indexer": indexer_payload,
            "application_logs": {
                "enabled": node.get_service::<neo_rpc::application_logs::ApplicationLogsService>().is_some(),
            },
            "tokens_tracker": {
                "enabled": node.get_service::<neo_rpc::plugins::tokens_tracker::TokensTrackerService>().is_some(),
            },
        },
    })
}

fn indexer_health_payload(node: &neo_system::Node, ledger_height: Option<u32>) -> (Value, bool) {
    match node.get_service::<neo_indexer::IndexerService>() {
        Some(indexer) => match indexer.try_status() {
            Ok(status) => (
                json!({
                    "enabled": true,
                    "ready": true,
                    "indexed_height": status.indexed_height,
                    "blocks_behind": status.blocks_behind(ledger_height),
                    "synced": status.is_synced_with(ledger_height),
                    "indexed_blocks": status.indexed_blocks,
                    "indexed_transactions": status.indexed_transactions,
                    "indexed_accounts": status.indexed_accounts,
                    "indexed_notifications": status.indexed_notifications,
                    "indexed_notification_accounts": status.indexed_notification_accounts,
                }),
                true,
            ),
            Err(err) => (
                json!({
                    "enabled": true,
                    "ready": false,
                    "error": err.to_string(),
                }),
                false,
            ),
        },
        None => (
            json!({
                "enabled": false,
                "ready": true,
            }),
            true,
        ),
    }
}
