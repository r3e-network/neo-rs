use std::any::TypeId;

/// Represents an attribute for reflection caching in the Neo ecosystem.
///
/// This attribute is used to mark fields that should be cached for reflection purposes.
/// It is typically used internally within the Neo Rust SDK.
#[derive(Debug, Clone)]
pub struct ReflectionCacheAttribute {
    /// The TypeId of the type to be cached
    type_id: TypeId,
}

impl ReflectionCacheAttribute {
    /// Creates a new ReflectionCacheAttribute.
    ///
    /// # Arguments
    ///
    /// * `type_id` - The TypeId of the type to be cached.
    pub fn new<T: 'static>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
        }
    }

    /// Gets the TypeId of the cached type.
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }
}