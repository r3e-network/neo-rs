// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_notification_event.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

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
mod tests {
    use super::*;

    #[test]
    fn notification_roundtrip() {
        let notif = RpcNotificationEvent {
            contract: "0x01".to_string(),
            event_name: "Evt".to_string(),
            state: serde_json::json!({"foo": "bar"}),
        };
        let json = serde_json::to_string(&notif).unwrap();
        let parsed: RpcNotificationEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.contract, notif.contract);
        assert_eq!(parsed.event_name, notif.event_name);
        assert_eq!(parsed.state["foo"], "bar");
    }
}
