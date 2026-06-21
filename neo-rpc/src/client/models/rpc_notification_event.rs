use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Represents a notification raised during smart contract execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcNotificationEvent {
    /// Contract script hash that produced the notification.
    pub contract: String,
    /// Event name supplied by the contract.
    pub event_name: String,
    /// Raw notification payload.
    #[serde(default)]
    pub state: Value,
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_notification_event.rs"]
mod tests;
