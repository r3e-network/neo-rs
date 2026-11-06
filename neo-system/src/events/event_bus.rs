//! EventBus: Tokio broadcast-based pub/sub system.
//!
//! Replaces the ActorRef-based EventStream with type-safe broadcast channels,
//! unblocking TaskManager and other actor migrations.

use neo_ledger::blockchain::types::{PersistCompleted, PersistFailed, RelayResult};
use neo_state_service::ValidatedRootPersisted;
use tokio::sync::broadcast;

/// Capacity for each event channel. Matches typical actor mailbox sizes.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Type-safe event bus using tokio broadcast channels.
///
/// Each event type has its own dedicated channel, providing compile-time
/// guarantees and avoiding the type erasure required by the actor-based approach.
#[derive(Clone)]
pub struct EventBus {
    persist_completed: broadcast::Sender<PersistCompleted>,
    persist_failed: broadcast::Sender<PersistFailed>,
    relay_result: broadcast::Sender<RelayResult>,
    validated_root_persisted: broadcast::Sender<ValidatedRootPersisted>,
}

impl EventBus {
    /// Creates a new EventBus with default channel capacities.
    pub fn new() -> Self {
        Self {
            persist_completed: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
            persist_failed: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
            relay_result: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
            validated_root_persisted: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
        }
    }

    // ==================== PersistCompleted ====================

    /// Subscribes to PersistCompleted events.
    ///
    /// Returns a receiver that will receive all future PersistCompleted events.
    /// The receiver will lag if events are published faster than consumed.
    pub fn subscribe_persist_completed(&self) -> broadcast::Receiver<PersistCompleted> {
        self.persist_completed.subscribe()
    }

    /// Publishes a PersistCompleted event to all subscribers.
    ///
    /// Lagging receivers will receive a `RecvError::Lagged` and miss events.
    pub fn publish_persist_completed(&self, event: PersistCompleted) {
        let _ = self.persist_completed.send(event);
    }

    // ==================== PersistFailed ====================

    /// Subscribes to PersistFailed events.
    pub fn subscribe_persist_failed(&self) -> broadcast::Receiver<PersistFailed> {
        self.persist_failed.subscribe()
    }

    /// Publishes a PersistFailed event to all subscribers.
    pub fn publish_persist_failed(&self, event: PersistFailed) {
        let _ = self.persist_failed.send(event);
    }

    // ==================== RelayResult ====================

    /// Subscribes to RelayResult events.
    pub fn subscribe_relay_result(&self) -> broadcast::Receiver<RelayResult> {
        self.relay_result.subscribe()
    }

    /// Publishes a RelayResult event to all subscribers.
    pub fn publish_relay_result(&self, event: RelayResult) {
        let _ = self.relay_result.send(event);
    }

    // ==================== ValidatedRootPersisted ====================

    /// Subscribes to ValidatedRootPersisted events.
    pub fn subscribe_validated_root_persisted(
        &self,
    ) -> broadcast::Receiver<ValidatedRootPersisted> {
        self.validated_root_persisted.subscribe()
    }

    /// Publishes a ValidatedRootPersisted event to all subscribers.
    pub fn publish_validated_root_persisted(&self, event: ValidatedRootPersisted) {
        let _ = self.validated_root_persisted.send(event);
    }

    // ==================== Utility ====================

    /// Returns the current subscriber count for PersistCompleted events.
    pub fn persist_completed_subscriber_count(&self) -> usize {
        self.persist_completed.receiver_count()
    }

    /// Returns the current subscriber count for RelayResult events.
    pub fn relay_result_subscriber_count(&self) -> usize {
        self.relay_result.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_persist_completed_pubsub() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_persist_completed();

        let event = PersistCompleted {
            block: Arc::new(Default::default()),
        };

        bus.publish_persist_completed(event.clone());

        let received = rx.recv().await.unwrap();
        assert!(Arc::ptr_eq(&received.block, &event.block));
    }

    #[tokio::test]
    async fn test_relay_result_pubsub() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_relay_result();

        let event = RelayResult {
            hash: Default::default(),
            inventory_type: neo_protocol::p2p::payloads::InventoryType::Block,
            block_index: Some(100),
            result: neo_ledger::VerifyResult::Succeed,
        };

        bus.publish_relay_result(event.clone());

        let received = rx.recv().await.unwrap();
        assert_eq!(received.hash, event.hash);
        assert_eq!(received.block_index, Some(100));
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe_persist_completed();
        let mut rx2 = bus.subscribe_persist_completed();

        assert_eq!(bus.persist_completed_subscriber_count(), 2);

        let event = PersistCompleted {
            block: Arc::new(Default::default()),
        };

        bus.publish_persist_completed(event.clone());

        let received1 = rx1.recv().await.unwrap();
        let received2 = rx2.recv().await.unwrap();

        assert!(Arc::ptr_eq(&received1.block, &event.block));
        assert!(Arc::ptr_eq(&received2.block, &event.block));
    }
}
