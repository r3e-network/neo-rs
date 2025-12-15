//! Service registry for typed and named service discovery.
//!
//! This module provides a thread-safe registry for storing and retrieving
//! services by type or name. It supports the dependency injection pattern
//! used throughout the Neo system.
//!
//! # Example
//!
//! ```ignore
//! use neo_core::neo_system::ServiceRegistry;
//! use std::sync::Arc;
//!
//! let registry = ServiceRegistry::new();
//!
//! // Register a typed service
//! registry.register_typed(Arc::new(MyService::new()))?;
//!
//! // Retrieve by type
//! let service = registry.get_typed::<MyService>()?;
//!
//! // Register a named service
//! registry.register_named("rpc", Arc::new(RpcService::new()))?;
//!
//! // Retrieve by name
//! let rpc = registry.get_named_service::<RpcService>("rpc")?;
//! ```

use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::error::CoreResult;

/// Attempts to downcast a service Arc to a concrete type.
fn downcast_service<T>(service: &Arc<dyn Any + Send + Sync>) -> Option<Arc<T>>
where
    T: Any + Send + Sync + 'static,
{
    Arc::downcast::<T>(Arc::clone(service)).ok()
}

/// Registry for storing and retrieving arbitrary services by type or name.
///
/// # Thread Safety
///
/// All operations are thread-safe. The registry uses `RwLock` for concurrent access.
///
/// # Lock Ordering
///
/// When acquiring multiple locks, always acquire in this order:
/// 1. `services_by_name`
/// 2. `typed_services`
/// 3. `services`
#[derive(Default)]
pub struct ServiceRegistry {
    services_by_name: RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>,
    typed_services: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    services: RwLock<Vec<Arc<dyn Any + Send + Sync>>>,
}

impl ServiceRegistry {
    /// Creates a new empty service registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a service with an optional name.
    ///
    /// The service is always registered by type. If a name is provided,
    /// it's also registered by name for string-based lookup.
    pub fn register(
        &self,
        service: Arc<dyn Any + Send + Sync>,
        name: Option<String>,
    ) -> CoreResult<()> {
        {
            let mut typed = self.typed_services.write();
            typed.insert((*service).type_id(), service.clone());
        }

        {
            let mut list = self.services.write();
            list.push(service.clone());
        }

        if let Some(ref name_ref) = name {
            let mut map = self.services_by_name.write();
            map.insert(name_ref.clone(), service.clone());
        }

        Ok(())
    }

    /// Registers a typed service without a name.
    #[allow(dead_code)]
    pub(crate) fn register_typed<T>(&self, service: Arc<T>) -> CoreResult<()>
    where
        T: Any + Send + Sync + 'static,
    {
        let any_arc: Arc<dyn Any + Send + Sync> = service;
        self.register(any_arc, None)
    }

    /// Retrieves a service by type, checking both typed registry and service list.
    pub fn get_service<T>(&self) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        if let Some(service) = self.get_typed::<T>()? {
            return Ok(Some(service));
        }
        let guard = self.services.read();
        Ok(guard.iter().find_map(downcast_service::<T>))
    }

    /// Retrieves a service by its exact type from the typed registry.
    pub fn get_typed<T>(&self) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        let guard = self.typed_services.read();
        if let Some(service) = guard.get(&TypeId::of::<T>()) {
            return Ok(downcast_service::<T>(service));
        }
        Ok(None)
    }

    /// Retrieves a service by name and attempts to downcast to the specified type.
    pub fn get_named_service<T>(&self, name: &str) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        let guard = self.services_by_name.read();
        Ok(guard.get(name).and_then(downcast_service::<T>))
    }

    /// Returns `true` if a service with the given name is registered.
    pub fn has_named_service(&self, name: &str) -> bool {
        self.services_by_name.read().contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct DummyService;

    #[test]
    fn typed_service_registry_registers_and_fetches_by_type() {
        let registry = ServiceRegistry::new();
        let service = Arc::new(DummyService);

        registry
            .register_typed(service.clone())
            .expect("typed registration");

        let fetched = registry
            .get_typed::<DummyService>()
            .expect("lookup should succeed")
            .expect("service should be present");

        assert!(Arc::ptr_eq(&service, &fetched));

        // Typed lookup should also surface through the legacy get_service helper.
        let fetched_via_get = registry
            .get_service::<DummyService>()
            .expect("fallback lookup")
            .expect("service should be present");
        assert!(Arc::ptr_eq(&service, &fetched_via_get));
    }

    #[test]
    fn named_service_lookup_works() {
        let registry = ServiceRegistry::new();
        let service: Arc<dyn Any + Send + Sync> = Arc::new(DummyService);

        registry
            .register(service.clone(), Some("TestService".to_string()))
            .expect("registration");

        assert!(registry.has_named_service("TestService"));
        assert!(!registry.has_named_service("NonExistent"));

        let fetched = registry
            .get_named_service::<DummyService>("TestService")
            .expect("lookup")
            .expect("service present");

        assert!(Arc::ptr_eq(
            &(service.clone() as Arc<dyn Any + Send + Sync>),
            &(fetched as Arc<dyn Any + Send + Sync>)
        ));
    }
}
