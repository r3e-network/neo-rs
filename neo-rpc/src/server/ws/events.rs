//! WebSocket event types and serialization

use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Types of events that can be subscribed to
macro_rules! ws_event_types {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $wire:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )+
        }

        impl $name {
            /// Returns the JSON-RPC/WebSocket wire name for this event type.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $wire,
                    )+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $(
                        $wire => Ok(Self::$variant),
                    )+
                    _ => Err(format!("unknown event type: {s}")),
                }
            }
        }
    };
}

ws_event_types! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum WsEventType {
        /// New block added to the chain
        BlockAdded => "block_added",
        /// Transaction added to mempool
        TransactionAdded => "transaction_added",
        /// Transaction(s) removed from mempool
        TransactionRemoved => "transaction_removed",
        /// Contract notification event
        Notification => "notification",
    }
}

fn prefixed_hash(hash: &UInt256) -> String {
    format!("0x{}", hex::encode(hash.as_bytes()))
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
            hash: prefixed_hash(hash),
            height,
        }
    }

    /// Create a transaction added event
    #[must_use]
    pub fn transaction_added(hash: &UInt256) -> Self {
        Self::TransactionAdded {
            hash: prefixed_hash(hash),
        }
    }

    /// Create a transaction removed event
    #[must_use]
    pub fn transaction_removed(hashes: &[UInt256], reason: &str) -> Self {
        Self::TransactionRemoved {
            hashes: hashes.iter().map(prefixed_hash).collect(),
            reason: reason.to_string(),
        }
    }

    /// Create a notification event
    #[must_use]
    pub fn notification(contract: &UInt256, event_name: &str, state: serde_json::Value) -> Self {
        Self::Notification {
            contract: prefixed_hash(contract),
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
            method: event.event_type().as_str().to_string(),
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
