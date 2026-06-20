//! Integration tests for websocket event notification JSON shapes.

use neo_primitives::UInt256;
use neo_rpc::server::{WsEvent, WsEventType, WsNotification};
use serde_json::json;

#[test]
fn websocket_event_type_wire_names_roundtrip() {
    let cases = [
        (WsEventType::BlockAdded, "block_added"),
        (WsEventType::TransactionAdded, "transaction_added"),
        (WsEventType::TransactionRemoved, "transaction_removed"),
        (WsEventType::Notification, "notification"),
    ];

    for (event_type, wire_name) in cases {
        assert_eq!(event_type.as_str(), wire_name);
        assert_eq!(event_type.to_string(), wire_name);
        assert_eq!(wire_name.parse::<WsEventType>(), Ok(event_type));
    }

    assert!("unknown".parse::<WsEventType>().is_err());
}

#[test]
fn websocket_notification_reuses_event_type_wire_name() {
    let event = WsEvent::TransactionRemoved {
        hashes: vec!["0x1234".to_string(), "0xabcd".to_string()],
        reason: "expired".to_string(),
    };

    let notification = WsNotification::from_event(&event);
    assert_eq!(
        notification.method,
        WsEventType::TransactionRemoved.as_str()
    );
    assert_eq!(
        notification.params,
        json!({ "hashes": ["0x1234", "0xabcd"], "reason": "expired" })
    );
}

#[test]
fn websocket_notification_keeps_neo_notification_field_names() {
    let event = WsEvent::Notification {
        contract: "0xfeed".to_string(),
        event_name: "Transfer".to_string(),
        state: json!([1, 2, 3]),
    };

    let notification = WsNotification::from_event(&event);
    assert_eq!(notification.method, WsEventType::Notification.as_str());
    assert_eq!(
        notification.params,
        json!({ "contract": "0xfeed", "eventname": "Transfer", "state": [1, 2, 3] })
    );
}

#[test]
fn websocket_event_constructors_prefix_hashes() {
    let hash = UInt256::from_bytes(&[0xabu8; 32]).expect("hash");
    let other_hash = UInt256::from_bytes(&[0xcdu8; 32]).expect("other hash");
    let expected = format!("0x{}", "ab".repeat(32));
    let other_expected = format!("0x{}", "cd".repeat(32));

    let block = WsEvent::block_added(&hash, 7);
    assert_eq!(
        WsNotification::from_event(&block).params,
        json!({ "hash": expected, "height": 7 })
    );

    let added = WsEvent::transaction_added(&hash);
    assert_eq!(
        WsNotification::from_event(&added).params,
        json!({ "hash": expected })
    );

    let removed = WsEvent::transaction_removed(&[hash, other_hash], "expired");
    assert_eq!(
        WsNotification::from_event(&removed).params,
        json!({ "hashes": [expected, other_expected], "reason": "expired" })
    );

    let notification = WsEvent::notification(&hash, "Transfer", json!([]));
    assert_eq!(
        WsNotification::from_event(&notification).params,
        json!({ "contract": expected, "eventname": "Transfer", "state": [] })
    );
}
