//! Message handler registry for remote node inbound notifications.
use crate::i_event_handlers::IMessageReceivedHandler;
use parking_lot::RwLock;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, OnceLock,
};

pub(crate) struct MessageHandlerEntry {
    pub(crate) id: usize,
    pub(crate) handler: Arc<dyn IMessageReceivedHandler + Send + Sync>,
}

static MESSAGE_HANDLERS: OnceLock<RwLock<Vec<MessageHandlerEntry>>> = OnceLock::new();
static NEXT_HANDLER_ID: AtomicUsize = AtomicUsize::new(1);

fn handler_registry() -> &'static RwLock<Vec<MessageHandlerEntry>> {
    MESSAGE_HANDLERS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Subscription handle returned when registering message-received callbacks.
#[derive(Debug)]
pub struct MessageHandlerSubscription {
    id: Option<usize>,
}

impl MessageHandlerSubscription {
    /// Explicitly unregisters the handler associated with this subscription.
    pub fn unregister(mut self) {
        if let Some(id) = self.id.take() {
            remove_handler(id);
        }
    }
}

impl Drop for MessageHandlerSubscription {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            remove_handler(id);
        }
    }
}

fn remove_handler(id: usize) {
    let mut handlers = handler_registry().write();
    handlers.retain(|entry| entry.id != id);
}

/// Registers a new message-received handler (parity with C# `RemoteNode.MessageReceived`).
pub fn register_message_received_handler(
    handler: Arc<dyn IMessageReceivedHandler + Send + Sync>,
) -> MessageHandlerSubscription {
    let id = NEXT_HANDLER_ID.fetch_add(1, Ordering::Relaxed);
    let entry = MessageHandlerEntry { id, handler };
    handler_registry().write().push(entry);
    MessageHandlerSubscription { id: Some(id) }
}

/// Removes a previously registered handler using its subscription token.
pub fn unregister_message_received_handler(subscription: MessageHandlerSubscription) {
    subscription.unregister();
}

pub(crate) fn with_handlers<T>(mut f: impl FnMut(&[MessageHandlerEntry]) -> T) -> T {
    f(&handler_registry().read())
}

#[cfg(test)]
pub(crate) fn reset() {
    handler_registry().write().clear();
}
