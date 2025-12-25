//! WebSocket event types and serialization

use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Types of events that can be subscribed to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsEventType {
    /// New block added to the chain
    BlockAdded,
    /// Transaction added to mempool
    TransactionAdded,
    /// Transaction(s) removed from mempool
    TransactionRemoved,
    /// Contract notification event
    Notification,
}

impl WsEventType {
    /// Parse event type from string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "block_added" => Some(Self::BlockAdded),
            "transaction_added" => Some(Self::TransactionAdded),
            "transaction_removed" => Some(Self::TransactionRemoved),
            "notification" => Some(Self::Notification),
            _ => None,
        }
    }
}

/// WebSocket event payload
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "method", content = "params")]
#[serde(rename_all = "snake_case")]
pub enum WsEvent {
    /// Block added event
    BlockAdded {
        /// Block hash (hex encoded with 0x prefix)
        hash: String,
        /// Block height
        height: u32,
    },
    /// Transaction added to mempool
    TransactionAdded {
        /// Transaction hash (hex encoded with 0x prefix)
        hash: String,
    },
    /// Transaction(s) removed from mempool
    TransactionRemoved {
        /// Transaction hashes (hex encoded with 0x prefix)
        hashes: Vec<String>,
        /// Removal reason
        reason: String,
    },
    /// Contract notification
    Notification {
        /// Contract script hash (hex encoded with 0x prefix)
        contract: String,
        /// Event name
        event_name: String,
        /// Event state (JSON value)
        state: serde_json::Value,
    },
}

impl WsEvent {
    /// Get the event type
    pub fn event_type(&self) -> WsEventType {
        match self {
            Self::BlockAdded { .. } => WsEventType::BlockAdded,
            Self::TransactionAdded { .. } => WsEventType::TransactionAdded,
            Self::TransactionRemoved { .. } => WsEventType::TransactionRemoved,
            Self::Notification { .. } => WsEventType::Notification,
        }
    }

    /// Create a block added event
    pub fn block_added(hash: &UInt256, height: u32) -> Self {
        Self::BlockAdded {
            hash: format!("0x{}", hex::encode(hash.as_bytes())),
            height,
        }
    }

    /// Create a transaction added event
    pub fn transaction_added(hash: &UInt256) -> Self {
        Self::TransactionAdded {
            hash: format!("0x{}", hex::encode(hash.as_bytes())),
        }
    }

    /// Create a transaction removed event
    pub fn transaction_removed(hashes: &[UInt256], reason: &str) -> Self {
        Self::TransactionRemoved {
            hashes: hashes
                .iter()
                .map(|h| format!("0x{}", hex::encode(h.as_bytes())))
                .collect(),
            reason: reason.to_string(),
        }
    }

    /// Create a notification event
    pub fn notification(contract: &UInt256, event_name: &str, state: serde_json::Value) -> Self {
        Self::Notification {
            contract: format!("0x{}", hex::encode(contract.as_bytes())),
            event_name: event_name.to_string(),
            state,
        }
    }
}

/// JSON-RPC 2.0 WebSocket notification message
#[derive(Debug, Serialize)]
pub struct WsNotification {
    /// JSON-RPC version
    pub jsonrpc: &'static str,
    /// Event method name
    pub method: String,
    /// Event parameters
    pub params: serde_json::Value,
}

impl WsNotification {
    /// Create a new notification from an event
    pub fn from_event(event: &WsEvent) -> Self {
        let method = match event {
            WsEvent::BlockAdded { .. } => "block_added",
            WsEvent::TransactionAdded { .. } => "transaction_added",
            WsEvent::TransactionRemoved { .. } => "transaction_removed",
            WsEvent::Notification { .. } => "notification",
        };

        let params = match event {
            WsEvent::BlockAdded { hash, height } => {
                serde_json::json!({ "hash": hash, "height": height })
            }
            WsEvent::TransactionAdded { hash } => {
                serde_json::json!({ "hash": hash })
            }
            WsEvent::TransactionRemoved { hashes, reason } => {
                serde_json::json!({ "hashes": hashes, "reason": reason })
            }
            WsEvent::Notification {
                contract,
                event_name,
                state,
            } => {
                serde_json::json!({ "contract": contract, "eventname": event_name, "state": state })
            }
        };

        Self {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
        }
    }

    /// Serialize to JSON string
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_parsing() {
        assert_eq!(
            WsEventType::parse("block_added"),
            Some(WsEventType::BlockAdded)
        );
        assert_eq!(
            WsEventType::parse("transaction_added"),
            Some(WsEventType::TransactionAdded)
        );
        assert_eq!(WsEventType::parse("unknown"), None);
    }

    #[test]
    fn test_notification_serialization() {
        let event = WsEvent::BlockAdded {
            hash: "0x1234".to_string(),
            height: 100,
        };
        let notification = WsNotification::from_event(&event);
        let json = notification.to_json();
        assert!(json.contains("block_added"));
        assert!(json.contains("100"));
    }
}
