//! Minimal type-map registry for optional node services.
//!
//! The legacy `NeoSystem` exposed `AddService` / `GetService<T>` so
//! optional plugin services (application logs, tokens tracker, oracle,
//! state service, …) could be discovered by the RPC server at request
//! time. This is a plain `TypeId → Arc<dyn Any>` map: the composition
//! root (node binary, test fixture) registers each service it has
//! started, and consumers look the service up by its concrete type.
//!
//! The registry deliberately stores **one instance per type** — the
//! same contract the C# `NeoSystem.GetService<T>()` default overload
//! provides — because every current consumer (RPC `listplugins`,
//! `getapplicationlog`, state queries, token-tracker queries) wants
//! "the" service of a given type, not a named collection.
//!
//! Lives in `neo-runtime` (L3) so that `neo-rpc` and other consumers
//! can use it without depending on `neo-system` (L5).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

/// Thread-safe, cheaply clonable type-map of `Arc`-shared services.
///
/// Cloning the registry clones the *handle*; all clones observe the
/// same underlying map, so a service registered through one clone is
/// visible through every other.
#[derive(Clone, Default)]
pub struct ServiceRegistry {
    inner: Arc<RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>>,
}

impl std::fmt::Debug for ServiceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceRegistry")
            .field("len", &self.inner.read().len())
            .finish()
    }
}

impl ServiceRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `service` as *the* instance of type `T`, replacing
    /// any previously registered instance of the same type. Returns
    /// the previous instance when one was replaced.
    pub fn register<T: Send + Sync + 'static>(&self, service: Arc<T>) -> Option<Arc<T>> {
        self.inner
            .write()
            .insert(TypeId::of::<T>(), service)
            .map(|previous| {
                previous
                    .downcast::<T>()
                    .expect("registry key is TypeId::of::<T>, so the value downcasts to T")
            })
    }

    /// Looks up the registered instance of type `T`, if any.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.inner.read().get(&TypeId::of::<T>()).map(|service| {
            Arc::clone(service)
                .downcast::<T>()
                .expect("registry key is TypeId::of::<T>, so the value downcasts to T")
        })
    }

    /// Returns whether an instance of type `T` is registered.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.inner.read().contains_key(&TypeId::of::<T>())
    }

    /// Number of registered services.
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Returns whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_get_service() {
        let registry = ServiceRegistry::new();
        let service: Arc<String> = Arc::new("hello".to_string());
        registry.register(service);
        let retrieved = registry.get::<String>();
        assert!(retrieved.is_some());
        assert_eq!(&**retrieved.unwrap(), "hello");
    }

    #[test]
    fn register_replaces_previous() {
        let registry = ServiceRegistry::new();
        registry.register::<String>(Arc::new("first".to_string()));
        let previous = registry.register::<String>(Arc::new("second".to_string()));
        assert!(previous.is_some());
        assert_eq!(&**previous.unwrap(), "first");
        assert_eq!(&**registry.get::<String>().unwrap(), "second");
    }

    #[test]
    fn get_missing_service_returns_none() {
        let registry = ServiceRegistry::new();
        assert!(registry.get::<String>().is_none());
        assert!(!registry.contains::<String>());
    }

    #[test]
    fn clones_share_state() {
        let registry = ServiceRegistry::new();
        let clone = registry.clone();
        registry.register::<u32>(Arc::new(42));
        assert_eq!(*clone.get::<u32>().unwrap(), 42);
    }
}
