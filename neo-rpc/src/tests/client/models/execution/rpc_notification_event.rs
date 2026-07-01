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
