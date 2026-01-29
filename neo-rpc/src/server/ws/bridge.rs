//! Event bridge for connecting chain events to WebSocket notifications
//!
//! This module provides utilities to bridge external event sources
//! (like blockchain events) to the WebSocket subscription system.

use super::events::WsEvent;
use neo_primitives::UInt256;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::debug;

/// Event bridge that forwards chain events to WebSocket clients
///
/// This bridge subscribes to an external event source and converts
/// events to `WsEvent` for WebSocket clients.
pub struct WsEventBridge {
    /// Sender for WebSocket events
    ws_sender: broadcast::Sender<WsEvent>,
}

impl WsEventBridge {
    /// Create a new event bridge
    ///
    /// # Arguments
    /// * `capacity` - Buffer capacity for the WebSocket broadcast channel
    #[must_use] 
    pub fn new(capacity: usize) -> Self {
        let (ws_sender, _) = broadcast::channel(capacity);
        Self { ws_sender }
    }

    /// Get a receiver for WebSocket events
    #[must_use] 
    pub fn subscribe(&self) -> broadcast::Receiver<WsEvent> {
        self.ws_sender.subscribe()
    }

    /// Get the sender for direct event publishing
    #[must_use] 
    pub fn sender(&self) -> broadcast::Sender<WsEvent> {
        self.ws_sender.clone()
    }

    /// Publish a block added event
    pub fn notify_block_added(&self, hash: &UInt256, height: u32) {
        let event = WsEvent::block_added(hash, height);
        if let Err(e) = self.ws_sender.send(event) {
            debug!("No WebSocket subscribers for block event: {}", e);
        }
    }

    /// Publish a transaction added event
    pub fn notify_transaction_added(&self, hash: &UInt256) {
        let event = WsEvent::transaction_added(hash);
        if let Err(e) = self.ws_sender.send(event) {
            debug!("No WebSocket subscribers for tx event: {}", e);
        }
    }

    /// Publish a transaction removed event
    pub fn notify_transaction_removed(&self, hashes: &[UInt256], reason: &str) {
        let event = WsEvent::transaction_removed(hashes, reason);
        if let Err(e) = self.ws_sender.send(event) {
            debug!("No WebSocket subscribers for tx removal event: {}", e);
        }
    }

    /// Publish a contract notification event
    pub fn notify_contract_event(
        &self,
        contract: &UInt256,
        event_name: &str,
        state: serde_json::Value,
    ) {
        let event = WsEvent::notification(contract, event_name, state);
        if let Err(e) = self.ws_sender.send(event) {
            debug!("No WebSocket subscribers for notification event: {}", e);
        }
    }

    /// Get the number of active WebSocket receivers
    #[must_use] 
    pub fn receiver_count(&self) -> usize {
        self.ws_sender.receiver_count()
    }
}

impl Default for WsEventBridge {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// Shared event bridge wrapped in Arc
pub type SharedWsEventBridge = Arc<WsEventBridge>;

#[cfg(test)]
mod tests {
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
}
