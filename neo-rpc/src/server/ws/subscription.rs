//! WebSocket subscription management

use super::events::WsEventType;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for a subscription
pub type SubscriptionId = u64;

/// Manages WebSocket subscriptions across all connections
pub struct SubscriptionManager {
    /// Next subscription ID to assign
    next_id: AtomicU64,
    /// Map of subscription ID to subscribed event types
    subscriptions: DashMap<SubscriptionId, HashSet<WsEventType>>,
}

impl SubscriptionManager {
    /// Create a new subscription manager
    #[must_use] 
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            subscriptions: DashMap::new(),
        }
    }

    /// Subscribe to the given event types
    ///
    /// Returns the subscription ID
    pub fn subscribe(&self, event_types: Vec<WsEventType>) -> SubscriptionId {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.subscriptions
            .insert(id, event_types.into_iter().collect());
        id
    }

    /// Add event types to an existing subscription
    pub fn add_events(&self, id: SubscriptionId, event_types: Vec<WsEventType>) -> bool {
        if let Some(mut entry) = self.subscriptions.get_mut(&id) {
            for event_type in event_types {
                entry.insert(event_type);
            }
            true
        } else {
            false
        }
    }

    /// Remove event types from an existing subscription
    pub fn remove_events(&self, id: SubscriptionId, event_types: &[WsEventType]) -> bool {
        if let Some(mut entry) = self.subscriptions.get_mut(&id) {
            for event_type in event_types {
                entry.remove(event_type);
            }
            true
        } else {
            false
        }
    }

    /// Unsubscribe and remove the subscription entirely
    pub fn unsubscribe(&self, id: SubscriptionId) -> bool {
        self.subscriptions.remove(&id).is_some()
    }

    /// Check if a subscription is interested in the given event type
    pub fn is_subscribed(&self, id: SubscriptionId, event_type: WsEventType) -> bool {
        self.subscriptions
            .get(&id)
            .is_some_and(|types| types.contains(&event_type))
    }

    /// Get all event types for a subscription
    pub fn get_subscribed_events(&self, id: SubscriptionId) -> Option<Vec<WsEventType>> {
        self.subscriptions
            .get(&id)
            .map(|types| types.iter().copied().collect())
    }

    /// Get total number of active subscriptions
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscribe_unsubscribe() {
        let manager = SubscriptionManager::new();

        let id = manager.subscribe(vec![WsEventType::BlockAdded]);
        assert!(manager.is_subscribed(id, WsEventType::BlockAdded));
        assert!(!manager.is_subscribed(id, WsEventType::TransactionAdded));

        assert!(manager.unsubscribe(id));
        assert!(!manager.is_subscribed(id, WsEventType::BlockAdded));
    }

    #[test]
    fn test_add_remove_events() {
        let manager = SubscriptionManager::new();

        let id = manager.subscribe(vec![WsEventType::BlockAdded]);

        // Add event
        manager.add_events(id, vec![WsEventType::TransactionAdded]);
        assert!(manager.is_subscribed(id, WsEventType::TransactionAdded));

        // Remove event
        manager.remove_events(id, &[WsEventType::BlockAdded]);
        assert!(!manager.is_subscribed(id, WsEventType::BlockAdded));
        assert!(manager.is_subscribed(id, WsEventType::TransactionAdded));
    }

    #[test]
    fn test_multiple_subscriptions() {
        let manager = SubscriptionManager::new();

        let id1 = manager.subscribe(vec![WsEventType::BlockAdded]);
        let id2 = manager.subscribe(vec![WsEventType::TransactionAdded]);

        assert_ne!(id1, id2);
        assert!(manager.is_subscribed(id1, WsEventType::BlockAdded));
        assert!(!manager.is_subscribed(id1, WsEventType::TransactionAdded));
        assert!(manager.is_subscribed(id2, WsEventType::TransactionAdded));
        assert!(!manager.is_subscribed(id2, WsEventType::BlockAdded));
    }
}
