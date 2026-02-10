//! WebSocket event types and serialization

use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

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

impl fmt::Display for WsEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlockAdded => write!(f, "block_added"),
            Self::TransactionAdded => write!(f, "transaction_added"),
            Self::TransactionRemoved => write!(f, "transaction_removed"),
            Self::Notification => write!(f, "notification"),
        }
    }
}

impl FromStr for WsEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "block_added" => Ok(Self::BlockAdded),
            "transaction_added" => Ok(Self::TransactionAdded),
            "transaction_removed" => Ok(Self::TransactionRemoved),
            "notification" => Ok(Self::Notification),
            _ => Err(format!("unknown event type: {s}")),
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
    #[must_use]
    pub const fn event_type(&self) -> WsEventType {
        match self {
            Self::BlockAdded { .. } => WsEventType::BlockAdded,
            Self::TransactionAdded { .. } => WsEventType::TransactionAdded,
            Self::TransactionRemoved { .. } => WsEventType::TransactionRemoved,
            Self::Notification { .. } => WsEventType::Notification,
        }
    }

    /// Create a block added event
    #[must_use]
    pub fn block_added(hash: &UInt256, height: u32) -> Self {
        Self::BlockAdded {
            hash: format!("0x{}", hex::encode(hash.as_bytes())),
            height,
        }
    }

    /// Create a transaction added event
    #[must_use]
    pub fn transaction_added(hash: &UInt256) -> Self {
        Self::TransactionAdded {
            hash: format!("0x{}", hex::encode(hash.as_bytes())),
        }
    }

    /// Create a transaction removed event
    #[must_use]
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
    #[must_use]
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
    #[must_use]
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
    #[must_use]
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
            "block_added".parse::<WsEventType>(),
            Ok(WsEventType::BlockAdded)
        );
        assert_eq!(
            "transaction_added".parse::<WsEventType>(),
            Ok(WsEventType::TransactionAdded)
        );
        assert!("unknown".parse::<WsEventType>().is_err());
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
