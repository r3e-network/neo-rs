//! WebSocket connection handler

use super::events::{WsEvent, WsEventType, WsNotification};
use super::subscription::{ConnectionSubscription, SubscriptionManager};
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
    params: Option<Vec<serde_json::Value>>}

/// JSON-RPC 2.0 WebSocket response
#[derive(Debug, Serialize)]
struct WsResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<WsError>}

#[derive(Debug, Serialize)]
struct WsError {
    code: i32,
    message: String}

impl WsResponse {
    const fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None}
   }

    fn error(id: Option<serde_json::Value>, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(WsError {
                code,
                message: message.to_string()})}
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
    let mut subscription: Option<ConnectionSubscription> = None;

    info!("WebSocket client connected");

    loop {
        tokio::select! {
            // Handle incoming WebSocket messages
            msg = rx.next() => {
                match msg {
                    Some(Ok(msg)) if msg.is_text() => {
                        let text = match msg.to_str() {
                            Ok(t) => t,
                            Err(()) => continue};

                        match serde_json::from_str::<WsRequest>(text) {
                            Ok(req) => {
                                let response = handle_request(&req, &subscription_mgr, &mut subscription);
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
                        if let Some(subscription) = &subscription {
                            if subscription.is_subscribed(ws_event.event_type()) {
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

    if let Some(subscription) = subscription {
        debug!("Dropped subscription {} on disconnect", subscription.id());
   }

    info!("WebSocket client disconnected");
}

/// Handle a JSON-RPC request
fn handle_request(
    req: &WsRequest,
    subscription_mgr: &SubscriptionManager,
    current_subscription: &mut Option<ConnectionSubscription>,
) -> WsResponse {
    if req.jsonrpc != "2.0" {
        return WsResponse::error(req.id.clone(), -32600, "Invalid JSON-RPC version");
   }

    match req.method.as_str() {
        "subscribe" => handle_subscribe(req, subscription_mgr, current_subscription),
        "unsubscribe" => handle_unsubscribe(req, current_subscription),
        _ => WsResponse::error(
            req.id.clone(),
            -32601,
            &format!("Method not found: {}", req.method),
        )}
}

fn event_type_names(event_types: impl IntoIterator<Item = WsEventType>) -> Vec<String> {
    event_types
        .into_iter()
        .map(|event_type| event_type.as_str().to_string())
        .collect()
}

fn parse_event_types(params: &[serde_json::Value]) -> Vec<WsEventType> {
    params
        .iter()
        .filter_map(|param| param.as_str()?.parse::<WsEventType>().ok())
        .collect()
}

fn parse_subscription_id(param: &serde_json::Value) -> Option<u64> {
    param
        .as_u64()
        .or_else(|| param.as_str()?.parse::<u64>().ok())
}

fn handle_subscribe(
    req: &WsRequest,
    subscription_mgr: &SubscriptionManager,
    current_subscription: &mut Option<ConnectionSubscription>,
) -> WsResponse {
    let event_types = req
        .params
        .as_deref()
        .map(parse_event_types)
        .unwrap_or_default();

    if event_types.is_empty() {
        return WsResponse::error(
            req.id.clone(),
            -32602,
            "Invalid params: no valid event types provided. Valid types: block_added, transaction_added, transaction_removed, notification",
        );
   }

    // If already subscribed, add to existing subscription
    if let Some(subscription) = current_subscription.as_mut() {
        subscription.add_events(event_types);
        let subscribed = event_type_names(subscription.subscribed_events());
        return WsResponse::success(
            req.id.clone(),
            serde_json::json!({
                "subscription_id": subscription.id(),
                "subscribed": subscribed}),
        );
   }

    // Create new subscription
    let subscribed = event_type_names(event_types.iter().copied());
    let subscription = subscription_mgr.subscribe(event_types);
    let id = subscription.id();
    *current_subscription = Some(subscription);

    WsResponse::success(
        req.id.clone(),
        serde_json::json!({
            "subscription_id": id,
            "subscribed": subscribed}),
    )
}

fn handle_unsubscribe(
    req: &WsRequest,
    current_subscription: &mut Option<ConnectionSubscription>,
) -> WsResponse {
    let Some(subscription) = current_subscription.as_mut() else {
        return WsResponse::error(req.id.clone(), -32602, "No active subscription");
   };

    // Check if specific event types to unsubscribe from
    if let Some(params) = req.params.as_deref() {
        if !params.is_empty() {
            if params.len() == 1 {
                if let Some(id) = parse_subscription_id(&params[0]) {
                    if id == subscription.id() {
                        *current_subscription = None;
                        return WsResponse::success(
                            req.id.clone(),
                            serde_json::json!({"unsubscribed": true}),
                        );
                   }

                    return WsResponse::error(
                        req.id.clone(),
                        -32602,
                        "Invalid params: subscription id does not match active subscription",
                    );
               }
           }

            let event_types = parse_event_types(params);

            if !event_types.is_empty() {
                subscription.remove_events(&event_types);

                // Check if any events remain
                if subscription.is_empty() {
                    *current_subscription = None;
                    return WsResponse::success(
                        req.id.clone(),
                        serde_json::json!({"unsubscribed": true}),
                    );
               }

                let remaining_names = event_type_names(subscription.subscribed_events());
                return WsResponse::success(
                    req.id.clone(),
                    serde_json::json!({
                        "unsubscribed": event_type_names(event_types),
                        "remaining": remaining_names}),
                );
           }

            return WsResponse::error(
                req.id.clone(),
                -32602,
                "Invalid params: no valid event types or subscription id provided",
            );
       }
   }

    // Unsubscribe from everything
    *current_subscription = None;

    WsResponse::success(req.id.clone(), serde_json::json!({"unsubscribed": true}))
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
            params: Some(vec![serde_json::json!("block_added")])};

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
            params: None};

        let response = handle_request(&req, &mgr, &mut sub_id);
        assert!(response.error.is_some());
   }

    #[test]
    fn repeated_subscribe_keeps_id_and_merges_events() {
        let mgr = SubscriptionManager::new();
        let mut subscription = None;

        let block_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "subscribe".to_string(),
            params: Some(vec![serde_json::json!("block_added")])};
        let block_response = handle_request(&block_req, &mgr, &mut subscription);
        let block_result = block_response.result.expect("subscribe result");
        let subscription_id = block_result["subscription_id"]
            .as_u64()
            .expect("subscription id");
        assert_eq!(subscription_id, 1);
        assert_eq!(
            block_result["subscribed"],
            serde_json::json!(["block_added"])
        );

        let tx_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "subscribe".to_string(),
            params: Some(vec![serde_json::json!("transaction_added")])};
        let tx_response = handle_request(&tx_req, &mgr, &mut subscription);
        let tx_result = tx_response.result.expect("merged subscribe result");

        assert_eq!(tx_result["subscription_id"].as_u64(), Some(subscription_id));
        let subscribed = tx_result["subscribed"].as_array().expect("subscribed list");
        assert!(subscribed.contains(&serde_json::json!("block_added")));
        assert!(subscribed.contains(&serde_json::json!("transaction_added")));

        let subscription = subscription.as_ref().expect("active subscription");
        assert!(subscription.is_subscribed(WsEventType::BlockAdded));
        assert!(subscription.is_subscribed(WsEventType::TransactionAdded));
   }

    #[test]
    fn partial_unsubscribe_keeps_remaining_events() {
        let mgr = SubscriptionManager::new();
        let mut subscription = None;

        let subscribe_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "subscribe".to_string(),
            params: Some(vec![
                serde_json::json!("block_added"),
                serde_json::json!("transaction_added"),
            ])};
        handle_request(&subscribe_req, &mgr, &mut subscription);

        let remove_block_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "unsubscribe".to_string(),
            params: Some(vec![serde_json::json!("block_added")])};
        let remove_block_response = handle_request(&remove_block_req, &mgr, &mut subscription);
        let remove_block_result = remove_block_response.result.expect("partial unsubscribe");

        assert_eq!(
            remove_block_result["unsubscribed"],
            serde_json::json!(["block_added"])
        );
        assert_eq!(
            remove_block_result["remaining"],
            serde_json::json!(["transaction_added"])
        );
        let remaining = subscription.as_ref().expect("remaining subscription");
        assert!(!remaining.is_subscribed(WsEventType::BlockAdded));
        assert!(remaining.is_subscribed(WsEventType::TransactionAdded));

        let remove_tx_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(3)),
            method: "unsubscribe".to_string(),
            params: Some(vec![serde_json::json!("transaction_added")])};
        let remove_tx_response = handle_request(&remove_tx_req, &mgr, &mut subscription);
        assert_eq!(
            remove_tx_response.result,
            Some(serde_json::json!({"unsubscribed": true}))
        );
        assert!(subscription.is_none());
   }

    #[test]
    fn unsubscribe_accepts_subscription_id() {
        let mgr = SubscriptionManager::new();
        let mut subscription = None;

        let subscribe_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "subscribe".to_string(),
            params: Some(vec![serde_json::json!("block_added")])};
        let subscribe_response = handle_request(&subscribe_req, &mgr, &mut subscription);
        let subscription_id =
            subscribe_response.result.expect("subscribe result")["subscription_id"]
                .as_u64()
                .expect("subscription id");

        let unsubscribe_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "unsubscribe".to_string(),
            params: Some(vec![serde_json::json!(subscription_id)])};
        let unsubscribe_response = handle_request(&unsubscribe_req, &mgr, &mut subscription);

        assert_eq!(
            unsubscribe_response.result,
            Some(serde_json::json!({"unsubscribed": true}))
        );
        assert!(subscription.is_none());
   }

    #[test]
    fn invalid_unsubscribe_params_do_not_clear_subscription() {
        let mgr = SubscriptionManager::new();
        let mut subscription = None;

        let subscribe_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "subscribe".to_string(),
            params: Some(vec![serde_json::json!("block_added")])};
        handle_request(&subscribe_req, &mgr, &mut subscription);

        let unsubscribe_req = WsRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "unsubscribe".to_string(),
            params: Some(vec![serde_json::json!("block_aded")])};
        let unsubscribe_response = handle_request(&unsubscribe_req, &mgr, &mut subscription);

        assert!(unsubscribe_response.error.is_some());
        assert!(
            subscription
                .as_ref()
                .expect("subscription remains active")
                .is_subscribed(WsEventType::BlockAdded)
        );
   }
}
