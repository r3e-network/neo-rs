//! Chain events and subscription system

use neo_primitives::UInt256;
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Chain event types
#[derive(Debug, Clone)]
pub enum ChainEvent {
    /// New block added to chain
    BlockAdded {
        /// Block hash
        hash: UInt256,
        /// Block height
        height: u32,
        /// Is this block on main chain
        on_main_chain: bool,
    },

    /// Chain tip changed
    TipChanged {
        /// New tip hash
        new_hash: UInt256,
        /// New tip height
        new_height: u32,
        /// Previous tip hash
        prev_hash: UInt256,
    },

    /// Chain reorganization occurred
    Reorganization {
        /// Common ancestor hash
        fork_point: UInt256,
        /// Disconnected block hashes (old chain)
        disconnected: Vec<UInt256>,
        /// Connected block hashes (new chain)
        connected: Vec<UInt256>,
    },

    /// Genesis block initialized
    GenesisInitialized {
        /// Genesis hash
        hash: UInt256,
    },
}

/// Subscriber for chain events
pub struct ChainEventSubscriber {
    /// Broadcast sender for events
    sender: broadcast::Sender<ChainEvent>,

    /// Number of active subscribers
    subscriber_count: Arc<RwLock<usize>>,
}

impl ChainEventSubscriber {
    /// Create a new chain event subscriber
    #[must_use] 
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            subscriber_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Subscribe to chain events
    #[must_use] 
    pub fn subscribe(&self) -> broadcast::Receiver<ChainEvent> {
        *self.subscriber_count.write() += 1;
        self.sender.subscribe()
    }

    /// Publish an event to all subscribers
    pub fn publish(&self, event: ChainEvent) {
        // Ignore send errors (no receivers)
        let _ = self.sender.send(event);
    }

    /// Get the number of active subscribers
    #[must_use] 
    pub fn subscriber_count(&self) -> usize {
        *self.subscriber_count.read()
    }

    /// Notify block added
    pub fn notify_block_added(&self, hash: UInt256, height: u32, on_main_chain: bool) {
        self.publish(ChainEvent::BlockAdded {
            hash,
            height,
            on_main_chain,
        });
    }

    /// Notify tip changed
    pub fn notify_tip_changed(&self, new_hash: UInt256, new_height: u32, prev_hash: UInt256) {
        self.publish(ChainEvent::TipChanged {
            new_hash,
            new_height,
            prev_hash,
        });
    }

    /// Notify reorganization
    pub fn notify_reorg(
        &self,
        fork_point: UInt256,
        disconnected: Vec<UInt256>,
        connected: Vec<UInt256>,
    ) {
        self.publish(ChainEvent::Reorganization {
            fork_point,
            disconnected,
            connected,
        });
    }

    /// Notify genesis initialized
    pub fn notify_genesis(&self, hash: UInt256) {
        self.publish(ChainEvent::GenesisInitialized { hash });
    }
}

impl Default for ChainEventSubscriber {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_subscribe_and_receive() {
        let subscriber = ChainEventSubscriber::new(16);
        let mut receiver = subscriber.subscribe();

        let hash = UInt256::from([1u8; 32]);
        subscriber.notify_block_added(hash, 100, true);

        let event = receiver.recv().await.unwrap();
        match event {
            ChainEvent::BlockAdded {
                hash: h,
                height,
                on_main_chain,
            } => {
                assert_eq!(h, hash);
                assert_eq!(height, 100);
                assert!(on_main_chain);
            }
            _ => panic!("Unexpected event type"),
        }
    }

    #[test]
    fn test_subscriber_count() {
        let subscriber = ChainEventSubscriber::new(16);
        assert_eq!(subscriber.subscriber_count(), 0);

        let _r1 = subscriber.subscribe();
        assert_eq!(subscriber.subscriber_count(), 1);

        let _r2 = subscriber.subscribe();
        assert_eq!(subscriber.subscriber_count(), 2);
    }
}
