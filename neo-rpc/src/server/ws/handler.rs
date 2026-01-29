//! WebSocket connection handler

use super::events::{WsEvent, WsEventType, WsNotification};
use super::subscription::{SubscriptionId, SubscriptionManager};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use warp::ws::{Message, WebSocket};

/// JSON-RPC 2.0 WebSocket request
#[derive(Debug, Deserialize)]
struct WsRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<Vec<String>>,
}

/// JSON-RPC 2.0 WebSocket response
#[derive(Debug, Serialize)]
struct WsResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<WsError>,
}

#[derive(Debug, Serialize)]
struct WsError {
    code: i32,
    message: String,
}

impl WsResponse {
    const fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<serde_json::Value>, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(WsError {
                code,
                message: message.to_string(),
            }),
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Handle a WebSocket connection
///
/// # Arguments
/// * `ws` - The WebSocket connection
/// * `event_rx` - Receiver for blockchain events
/// * `subscription_mgr` - Shared subscription manager
pub async fn ws_handler(
    ws: WebSocket,
    mut event_rx: broadcast::Receiver<WsEvent>,
    subscription_mgr: Arc<SubscriptionManager>,
) {
    let (mut tx, mut rx) = ws.split();
    let mut subscription_id: Option<SubscriptionId> = None;

    info!("WebSocket client connected");

    loop {
        tokio::select! {
            // Handle incoming WebSocket messages
            msg = rx.next() => {
                match msg {
                    Some(Ok(msg)) if msg.is_text() => {
                        let text = match msg.to_str() {
                            Ok(t) => t,
                            Err(()) => continue,
                        };

                        match serde_json::from_str::<WsRequest>(text) {
                            Ok(req) => {
                                let response = handle_request(&req, &subscription_mgr, &mut subscription_id);
                                if let Err(e) = tx.send(Message::text(response.to_json())).await {
                                    warn!("Failed to send WebSocket response: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                let response = WsResponse::error(None, -32700, &format!("Parse error: {e}"));
                                if let Err(e) = tx.send(Message::text(response.to_json())).await {
                                    warn!("Failed to send WebSocket error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(msg)) if msg.is_close() => {
                        debug!("WebSocket client sent close");
                        break;
                    }
                    Some(Ok(msg)) if msg.is_ping() => {
                        if let Err(e) = tx.send(Message::pong(msg.into_bytes())).await {
                            warn!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        warn!("WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        debug!("WebSocket stream ended");
                        break;
                    }
                    _ => {}
                }
            }

            // Forward matching blockchain events
            event = event_rx.recv() => {
                match event {
                    Ok(ws_event) => {
                        if let Some(id) = subscription_id {
                            if subscription_mgr.is_subscribed(id, ws_event.event_type()) {
                                let notification = WsNotification::from_event(&ws_event);
                                if let Err(e) = tx.send(Message::text(notification.to_json())).await {
                                    warn!("Failed to send event notification: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket client lagged by {} events", n);
                        // Continue anyway, just log the lag
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Event channel closed");
                        break;
                    }
                }
            }
        }
    }

    // Cleanup subscription on disconnect
    if let Some(id) = subscription_id {
        subscription_mgr.unsubscribe(id);
        debug!("Cleaned up subscription {} on disconnect", id);
    }

    info!("WebSocket client disconnected");
}

/// Handle a JSON-RPC request
fn handle_request(
    req: &WsRequest,
    subscription_mgr: &SubscriptionManager,
    current_subscription: &mut Option<SubscriptionId>,
) -> WsResponse {
    if req.jsonrpc != "2.0" {
        return WsResponse::error(req.id.clone(), -32600, "Invalid JSON-RPC version");
    }

    match req.method.as_str() {
        "subscribe" => handle_subscribe(req, subscription_mgr, current_subscription),
        "unsubscribe" => handle_unsubscribe(req, subscription_mgr, current_subscription),
        _ => WsResponse::error(
            req.id.clone(),
            -32601,
            &format!("Method not found: {}", req.method),
        ),
    }
}

fn handle_subscribe(
    req: &WsRequest,
    subscription_mgr: &SubscriptionManager,
    current_subscription: &mut Option<SubscriptionId>,
) -> WsResponse {
    let event_types: Vec<WsEventType> = req
        .params
        .as_ref()
        .map(|params| {
            params
                .iter()
                .filter_map(|s| WsEventType::parse(s))
                .collect()
        })
        .unwrap_or_default();

    if event_types.is_empty() {
        return WsResponse::error(
            req.id.clone(),
            -32602,
            "Invalid params: no valid event types provided. Valid types: block_added, transaction_added, transaction_removed, notification",
        );
    }

    // If already subscribed, add to existing subscription
    if let Some(id) = *current_subscription {
        subscription_mgr.add_events(id, event_types);
        let subscribed: Vec<String> = subscription_mgr
            .get_subscribed_events(id)
            .unwrap_or_default()
            .iter()
            .map(|e| format!("{e:?}").to_lowercase())
            .collect();
        return WsResponse::success(
            req.id.clone(),
            serde_json::json!({
                "subscription_id": id,
                "subscribed": subscribed,
            }),
        );
    }

    // Create new subscription
    let id = subscription_mgr.subscribe(event_types.clone());
    *current_subscription = Some(id);

    let subscribed: Vec<String> = event_types
        .iter()
        .map(|e| format!("{e:?}").to_lowercase())
        .collect();

    WsResponse::success(
        req.id.clone(),
        serde_json::json!({
            "subscription_id": id,
            "subscribed": subscribed,
        }),
    )
}

fn handle_unsubscribe(
    req: &WsRequest,
    subscription_mgr: &SubscriptionManager,
    current_subscription: &mut Option<SubscriptionId>,
) -> WsResponse {
    let Some(id) = *current_subscription else {
        return WsResponse::error(req.id.clone(), -32602, "No active subscription");
    };

    // Check if specific event types to unsubscribe from
    if let Some(params) = &req.params {
        if !params.is_empty() {
            let event_types: Vec<WsEventType> = params
                .iter()
                .filter_map(|s| WsEventType::parse(s))
                .collect();

            if !event_types.is_empty() {
                subscription_mgr.remove_events(id, &event_types);

                // Check if any events remain
                let remaining = subscription_mgr.get_subscribed_events(id);
                if remaining.as_ref().map_or(true, std::vec::Vec::is_empty) {
                    subscription_mgr.unsubscribe(id);
                    *current_subscription = None;
                    return WsResponse::success(
                        req.id.clone(),
                        serde_json::json!({ "unsubscribed": true }),
                    );
                }

                let remaining_names: Vec<String> = remaining
                    .unwrap_or_default()
                    .iter()
                    .map(|e| format!("{e:?}").to_lowercase())
                    .collect();
                return WsResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "unsubscribed": event_types.iter().map(|e| format!("{e:?}").to_lowercase()).collect::<Vec<_>>(),
                        "remaining": remaining_names,
                    }),
                );
            }
        }
    }

    // Unsubscribe from everything
    subscription_mgr.unsubscribe(id);
    *current_subscription = None;

    WsResponse::success(req.id.clone(), serde_json::json!({ "unsubscribed": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_subscribe() {
        let mgr = SubscriptionManager::new();
        let mut sub_id = None;

        let req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "subscribe".to_string(),
            params: Some(vec!["block_added".to_string()]),
        };

        let response = handle_request(&req, &mgr, &mut sub_id);
        assert!(response.result.is_some());
        assert!(sub_id.is_some());
    }

    #[test]
    fn test_handle_invalid_method() {
        let mgr = SubscriptionManager::new();
        let mut sub_id = None;

        let req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "invalid".to_string(),
            params: None,
        };

        let response = handle_request(&req, &mgr, &mut sub_id);
        assert!(response.error.is_some());
    }
}
