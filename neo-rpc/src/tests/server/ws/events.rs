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
