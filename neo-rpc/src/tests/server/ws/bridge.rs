use super::*;

#[tokio::test]
async fn test_bridge_block_notification() {
    let bridge = WsEventBridge::new(16);
    let mut rx = bridge.subscribe();

    let hash = UInt256::from([0xABu8; 32]);
    bridge.notify_block_added(&hash, 12345);

    let event = rx.recv().await.unwrap();
    match event {
        WsEvent::BlockAdded { height, .. } => {
            assert_eq!(height, 12345);
        }
        _ => panic!("Expected BlockAdded event"),
    }
}

#[tokio::test]
async fn test_bridge_transaction_notification() {
    let bridge = WsEventBridge::new(16);
    let mut rx = bridge.subscribe();

    let hash = UInt256::from([0xCDu8; 32]);
    bridge.notify_transaction_added(&hash);

    let event = rx.recv().await.unwrap();
    assert!(matches!(event, WsEvent::TransactionAdded { .. }));
}

#[test]
fn test_bridge_receiver_count() {
    let bridge = WsEventBridge::new(16);
    assert_eq!(bridge.receiver_count(), 0);

    let _rx1 = bridge.subscribe();
    assert_eq!(bridge.receiver_count(), 1);

    let _rx2 = bridge.subscribe();
    assert_eq!(bridge.receiver_count(), 2);
}
