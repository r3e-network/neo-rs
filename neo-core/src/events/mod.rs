//! Simple event manager mirroring the C# `Neo.Events` helpers used by the
//! unit tests. Provides registration and triggering of typed handlers.
//!
//! Also provides lightweight `PluginEvent` for internal event broadcasting,
//! replacing the removed plugin system with simple logging.

use parking_lot::RwLock;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

/// Lightweight plugin event enum for internal event broadcasting.
/// Replaces the previous plugin system with simple logging.
pub enum PluginEvent {
    /// Node has started with system reference
    NodeStarted {
        /// Reference to the NeoSystem (or other system implementation)
        system: Arc<dyn Any + Send + Sync>,
    },
    /// Node is stopping
    NodeStopping,
    /// A block was received
    BlockReceived { block_hash: String, block_height: u32 },
    /// A transaction was received
    TransactionReceived { tx_hash: String },
    /// Transaction added to mempool
    MempoolTransactionAdded { tx_hash: String },
    /// Transactions removed from mempool
    MempoolTransactionRemoved { tx_hashes: Vec<String>, reason: String },
    /// A service was added
    ServiceAdded { service_name: String },
    /// Wallet changed
    WalletChanged { wallet_name: String },
}

impl std::fmt::Debug for PluginEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginEvent::NodeStarted { .. } => write!(f, "NodeStarted {{ system: ... }}"),
            PluginEvent::NodeStopping => write!(f, "NodeStopping"),
            PluginEvent::BlockReceived { block_hash, block_height } => {
                f.debug_struct("BlockReceived")
                    .field("block_hash", block_hash)
                    .field("block_height", block_height)
                    .finish()
            }
            PluginEvent::TransactionReceived { tx_hash } => {
                f.debug_struct("TransactionReceived")
                    .field("tx_hash", tx_hash)
                    .finish()
            }
            PluginEvent::MempoolTransactionAdded { tx_hash } => {
                f.debug_struct("MempoolTransactionAdded")
                    .field("tx_hash", tx_hash)
                    .finish()
            }
            PluginEvent::MempoolTransactionRemoved { tx_hashes, reason } => {
                f.debug_struct("MempoolTransactionRemoved")
                    .field("tx_hashes", tx_hashes)
                    .field("reason", reason)
                    .finish()
            }
            PluginEvent::ServiceAdded { service_name } => {
                f.debug_struct("ServiceAdded")
                    .field("service_name", service_name)
                    .finish()
            }
            PluginEvent::WalletChanged { wallet_name } => {
                f.debug_struct("WalletChanged")
                    .field("wallet_name", wallet_name)
                    .finish()
            }
        }
    }
}

impl std::fmt::Display for PluginEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginEvent::NodeStarted { .. } => write!(f, "NodeStarted"),
            PluginEvent::NodeStopping => write!(f, "NodeStopping"),
            PluginEvent::BlockReceived { block_hash, block_height } => {
                write!(f, "BlockReceived({}, height={})", block_hash, block_height)
            }
            PluginEvent::TransactionReceived { tx_hash } => {
                write!(f, "TransactionReceived({})", tx_hash)
            }
            PluginEvent::MempoolTransactionAdded { tx_hash } => {
                write!(f, "MempoolTransactionAdded({})", tx_hash)
            }
            PluginEvent::MempoolTransactionRemoved { tx_hashes, reason } => {
                write!(f, "MempoolTransactionRemoved({:?}, {})", tx_hashes, reason)
            }
            PluginEvent::ServiceAdded { service_name } => {
                write!(f, "ServiceAdded({})", service_name)
            }
            PluginEvent::WalletChanged { wallet_name } => {
                write!(f, "WalletChanged({})", wallet_name)
            }
        }
    }
}

/// Broadcasts a plugin event by logging it.
/// This is a no-op replacement for the removed plugin system.
#[inline]
pub fn broadcast_plugin_event(event: &PluginEvent) {
    debug!(target: "neo::events", event = %event, "plugin event");
}

/// Trait implemented by event handlers. Matches the behaviour used by the
/// C# `EventHandler` interface where handlers are invoked with a sender and
/// arbitrary arguments.
pub trait EventHandler: Send + Sync + 'static {
    fn handle(&self, sender: &dyn Any, args: &dyn Any);
}

/// Thread-safe manager for named events. This is a lightweight port of the
/// C# `EventManager` utility used by integration tests.
#[derive(Default)]
pub struct EventManager {
    handlers: RwLock<HashMap<String, Vec<Arc<dyn EventHandler>>>>,
}

impl EventManager {
    /// Creates a new, empty manager.
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// Registers an event handler for the specified event name.
    /// Returns `true` if the handler was added successfully.
    pub fn register<H>(&self, event: &str, handler: H) -> bool
    where
        H: EventHandler,
    {
        let mut handlers = self.handlers.write();
        handlers
            .entry(event.to_string())
            .or_default()
            .push(Arc::new(handler));
        true
    }

    /// Removes all handlers registered for the specified event. Returns
    /// `true` if a handler list existed and was removed.
    pub fn unregister(&self, event: &str) -> bool {
        let mut handlers = self.handlers.write();
        handlers.remove(event).is_some()
    }

    /// Triggers an event, invoking all registered handlers. Handlers are
    /// executed synchronously on the current thread.
    pub fn trigger(&self, event: &str, sender: &dyn Any, args: &dyn Any) {
        let maybe_handlers = {
            let handlers = self.handlers.read();
            handlers.get(event).cloned()
        };

        if let Some(list) = maybe_handlers {
            for handler in list {
                handler.handle(sender, args);
            }
        }
    }
}
