//! ReflectionCacheAttribute - matches C# Neo.IO.Caching.ReflectionCacheAttribute exactly

use std::any::TypeId;

/// Attribute to mark types for reflection caching (matches C# ReflectionCacheAttribute).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReflectionCacheAttribute {
    type_id: TypeId,
}

impl ReflectionCacheAttribute {
    /// Creates a new ReflectionCacheAttribute for the provided type identifier.
    pub fn new(type_id: TypeId) -> Self {
        Self { type_id }
    }

    /// Convenience helper mirroring C# constructor usage with typeof(T).
    pub fn of<T: 'static>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
        }
    }

    /// Gets the underlying type identifier.
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }
}
