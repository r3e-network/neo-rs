//! Simple event manager mirroring the C# `Neo.Events` helpers used by the
//! unit tests. Provides registration and triggering of typed handlers.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
        let mut handlers = self.handlers.write().expect("event registry poisoned");
        handlers
            .entry(event.to_string())
            .or_default()
            .push(Arc::new(handler));
        true
    }

    /// Removes all handlers registered for the specified event. Returns
    /// `true` if a handler list existed and was removed.
    pub fn unregister(&self, event: &str) -> bool {
        let mut handlers = self.handlers.write().expect("event registry poisoned");
        handlers.remove(event).is_some()
    }

    /// Triggers an event, invoking all registered handlers. Handlers are
    /// executed synchronously on the current thread.
    pub fn trigger(&self, event: &str, sender: &dyn Any, args: &dyn Any) {
        let maybe_handlers = {
            let handlers = self.handlers.read().expect("event registry poisoned");
            handlers.get(event).cloned()
        };

        if let Some(list) = maybe_handlers {
            for handler in list {
                handler.handle(sender, args);
            }
        }
    }
}
