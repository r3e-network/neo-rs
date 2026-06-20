use super::super::super::health::node_health_payload;
use super::support::{indexed_service_at, seed_ledger_height, test_node};

#[test]
fn node_health_payload_reports_disabled_optional_services() {
    let node = test_node();

    let payload = node_health_payload(&node);

    assert_eq!(payload["status"], "starting");
    assert_eq!(payload["ready"], false);
    assert!(payload["ledger_height"].is_null());
    assert_eq!(payload["connected_peers"], 0);
    assert_eq!(payload["mempool"]["transactions"], 0);
    assert_eq!(payload["services"]["indexer"]["enabled"], false);
    assert_eq!(payload["services"]["indexer"]["ready"], true);
    assert_eq!(payload["services"]["application_logs"]["enabled"], false);
    assert_eq!(payload["services"]["tokens_tracker"]["enabled"], false);
}

#[test]
fn node_health_payload_reports_indexer_sync_state_for_heartbeats() {
    let node = test_node();
    seed_ledger_height(&node, 5);
    node.register_service(indexed_service_at(5));

    let payload = node_health_payload(&node);
    let indexer = &payload["services"]["indexer"];

    assert_eq!(payload["status"], "ready");
    assert_eq!(indexer["enabled"], true);
    assert_eq!(indexer["ready"], true);
    assert_eq!(indexer["indexed_height"], 5);
    assert_eq!(indexer["blocks_behind"], 0);
    assert_eq!(indexer["synced"], true);
    assert_eq!(indexer["indexed_blocks"], 1);
    assert_eq!(indexer["indexed_transactions"], 0);
    assert_eq!(indexer["indexed_accounts"], 0);
    assert_eq!(indexer["indexed_notifications"], 0);
    assert_eq!(indexer["indexed_notification_accounts"], 0);
}

#[test]
fn node_health_payload_reports_indexer_ahead_of_ledger_as_unsynced() {
    let node = test_node();
    seed_ledger_height(&node, 3);
    node.register_service(indexed_service_at(5));

    let payload = node_health_payload(&node);
    let indexer = &payload["services"]["indexer"];

    assert_eq!(indexer["indexed_height"], 5);
    assert_eq!(indexer["blocks_behind"], 0);
    assert_eq!(indexer["synced"], false);
}
