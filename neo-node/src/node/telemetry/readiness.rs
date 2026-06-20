//! Readiness payloads shared by `/readyz` and Prometheus exporter state.

use hyper::{Body, Response, StatusCode};
use serde_json::{Value, json};

use super::http::json_response;

pub(super) struct ReadinessSnapshot {
    pub(super) ready: bool,
    pub(super) network_label: String,
    pub(super) ledger_height: Option<u32>,
    pub(super) connected_peers: usize,
    pub(super) mempool_transactions: usize,
    pub(super) header_cache_entries: usize,
    pub(super) state_service_enabled: bool,
    pub(super) indexer: ServiceReadiness,
    pub(super) application_logs_enabled: bool,
    pub(super) tokens_tracker_enabled: bool,
}

pub(super) struct ServiceReadiness {
    pub(super) ready: bool,
    pub(super) payload: Value,
}

pub(super) fn readiness_response(snapshot: ReadinessSnapshot) -> Response<Body> {
    json_response(
        if snapshot.ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        json!({
            "status": if snapshot.ready { "ready" } else { "starting" },
            "ready": snapshot.ready,
            "service": "neo-node",
            "version": env!("CARGO_PKG_VERSION"),
            "network": snapshot.network_label,
            "ledger_height": snapshot.ledger_height,
            "connected_peers": snapshot.connected_peers,
            "mempool_transactions": snapshot.mempool_transactions,
            "header_cache_entries": snapshot.header_cache_entries,
            "services": {
                "state_service": {"enabled": snapshot.state_service_enabled},
                "indexer": snapshot.indexer.payload,
                "application_logs": {"enabled": snapshot.application_logs_enabled},
                "tokens_tracker": {"enabled": snapshot.tokens_tracker_enabled},
            },
        }),
    )
}

pub(super) fn indexer_readiness(
    ledger_height: Option<u32>,
    status: Option<Result<neo_indexer::IndexerStatus, String>>,
) -> ServiceReadiness {
    match status {
        Some(Ok(status)) => ServiceReadiness {
            ready: true,
            payload: json!({
                "enabled": true,
                "ready": true,
                "ledger_height": ledger_height,
                "indexed_height": status.indexed_height,
                "blocks_behind": status.blocks_behind(ledger_height),
                "synced": status.is_synced_with(ledger_height),
                "indexed_blocks": status.indexed_blocks,
                "indexed_transactions": status.indexed_transactions,
                "indexed_accounts": status.indexed_accounts,
                "indexed_notifications": status.indexed_notifications,
                "indexed_notification_accounts": status.indexed_notification_accounts,
            }),
        },
        Some(Err(error)) => ServiceReadiness {
            ready: false,
            payload: json!({
                "enabled": true,
                "ready": false,
                "error": error,
            }),
        },
        None => ServiceReadiness {
            ready: true,
            payload: json!({
                "enabled": false,
                "ready": true,
            }),
        },
    }
}
