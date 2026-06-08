//! WebSocket subscription management

use super::events::WsEventType;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Unique identifier for a subscription
pub type SubscriptionId = u64;

/// Allocates WebSocket subscription identifiers.
pub struct SubscriptionManager {
    /// Next subscription ID to assign
    next_id: AtomicU64}

impl SubscriptionManager {
    /// Create a new subscription ID allocator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1)}
   }

    /// Create connection-local subscription state for the given event types.
    pub(super) fn subscribe(&self, event_types: Vec<WsEventType>) -> ConnectionSubscription {
        let id = self.next_id.fetch_add(1, Relaxed);
        ConnectionSubscription::new(id, event_types)
   }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
   }
}

/// Event subscriptions owned by one WebSocket connection.
pub(super) struct ConnectionSubscription {
    id: SubscriptionId,
    event_types: HashSet<WsEventType>}

impl ConnectionSubscription {
    fn new(id: SubscriptionId, event_types: Vec<WsEventType>) -> Self {
        Self {
            id,
            event_types: event_types.into_iter().collect()}
   }

    /// Return the connection-local subscription identifier.
    #[must_use]
    pub(super) const fn id(&self) -> SubscriptionId {
        self.id
   }

    /// Add event types to this subscription.
    pub(super) fn add_events(&mut self, event_types: Vec<WsEventType>) {
        for event_type in event_types {
            self.event_types.insert(event_type);
       }
   }

    /// Remove event types from this subscription.
    pub(super) fn remove_events(&mut self, event_types: &[WsEventType]) {
        for event_type in event_types {
            self.event_types.remove(event_type);
       }
   }

    /// Check if this subscription is interested in the given event type.
    pub(super) fn is_subscribed(&self, event_type: WsEventType) -> bool {
        self.event_types.contains(&event_type)
   }

    /// Get all event types for this subscription in wire-format order.
    pub(super) fn subscribed_events(&self) -> impl Iterator<Item = WsEventType> + '_ {
        WsEventType::ALL
            .iter()
            .copied()
            .filter(|event_type| self.event_types.contains(event_type))
   }

    /// Return whether this subscription has no event types left.
    pub(super) fn is_empty(&self) -> bool {
        self.event_types.is_empty()
   }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscribe_unsubscribe() {
        let manager = SubscriptionManager::new();

        let mut subscription = manager.subscribe(vec![WsEventType::BlockAdded]);
        assert!(subscription.is_subscribed(WsEventType::BlockAdded));
        assert!(!subscription.is_subscribed(WsEventType::TransactionAdded));

        subscription.remove_events(&[WsEventType::BlockAdded]);
        assert!(!subscription.is_subscribed(WsEventType::BlockAdded));
        assert!(subscription.is_empty());
   }

    #[test]
    fn test_add_remove_events() {
        let manager = SubscriptionManager::new();

        let mut subscription = manager.subscribe(vec![WsEventType::BlockAdded]);

        // Add event
        subscription.add_events(vec![WsEventType::TransactionAdded]);
        assert!(subscription.is_subscribed(WsEventType::TransactionAdded));

        // Remove event
        subscription.remove_events(&[WsEventType::BlockAdded]);
        assert!(!subscription.is_subscribed(WsEventType::BlockAdded));
        assert!(subscription.is_subscribed(WsEventType::TransactionAdded));
   }

    #[test]
    fn test_multiple_subscriptions() {
        let manager = SubscriptionManager::new();

        let subscription1 = manager.subscribe(vec![WsEventType::BlockAdded]);
        let subscription2 = manager.subscribe(vec![WsEventType::TransactionAdded]);

        assert_ne!(subscription1.id(), subscription2.id());
        assert!(subscription1.is_subscribed(WsEventType::BlockAdded));
        assert!(!subscription1.is_subscribed(WsEventType::TransactionAdded));
        assert!(subscription2.is_subscribed(WsEventType::TransactionAdded));
        assert!(!subscription2.is_subscribed(WsEventType::BlockAdded));
   }
}
