use std::any::Any;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::marker::PhantomData;
use crate::io::memory_reader::MemoryReader;
use crate::io::serializable_trait::SerializableTrait;
use lazy_static::lazy_static;
use std::sync::Mutex;

/// The `ReflectionCache` struct emulates a reflection cache, mapping keys to types,
/// and allows for creating instances of those types.
pub struct ReflectionCache<T>
where
    T: Copy + Eq + std::hash::Hash,
{
    cache: HashMap<
        T,
        (
            fn() -> Box<dyn Any>,
            fn(&mut MemoryReader) -> Result<Box<dyn SerializableTrait>, std::io::Error>,
        ),
    >,
    _marker: PhantomData<T>,
}

impl<T> ReflectionCache<T>
where
    T: Copy + Eq + std::hash::Hash,
{
    /// Creates a new `ReflectionCache`.
    pub fn new() -> Self {
        ReflectionCache {
            cache: HashMap::new(),
            _marker: PhantomData,
        }
    }

    /// Registers a type with a key.
    pub fn register<U>(&mut self, key: T)
    where
        U: 'static + Default + SerializableTrait,
    {
        self.cache.insert(
            key,
            (
                // Factory function to create a default instance
                || Box::new(U::default()),
                // Function to deserialize an instance from a MemoryReader
                |reader: &mut MemoryReader| -> Result<Box<dyn SerializableTrait>, std::io::Error> {
                    let instance = U::deserialize(reader)?;
                    Ok(Box::new(instance))
                },
            ),
        );
    }

    /// Creates an instance of the type associated with the given key.
    pub fn create_instance(&self, key: T) -> Option<Box<dyn Any>> {
        self.cache.get(&key).map(|(factory, _)| factory())
    }

    /// Creates an instance of the serializable type associated with the given key from a `MemoryReader`.
    pub fn create_serializable(
        &self,
        key: T,
        reader: &mut MemoryReader,
    ) -> Option<Result<Box<dyn SerializableTrait>, std::io::Error>> {
        self.cache.get(&key).map(|(_, from_reader)| from_reader(reader))
    }
}

use once_cell::sync::Lazy;

pub static GLOBAL_REFLECTION_CACHES: Lazy<Mutex<HashMap<std::any::TypeId, Box<dyn Any + Send + Sync>>>> = Lazy::new(|| {
    Mutex::new(HashMap::new())
});

pub fn get_or_create_global_reflection_cache<T: 'static + Copy + Eq + std::hash::Hash + Send + Sync>() -> &'static Mutex<ReflectionCache<T>> {
    let type_id = std::any::TypeId::of::<T>();
    let mut caches = GLOBAL_REFLECTION_CACHES.lock().unwrap();
    
    caches.entry(type_id).or_insert_with(|| {
        Box::new(Mutex::new(ReflectionCache::<T>::new()))
    });
    
    // Safety: We know this cast is safe because we just ensured the entry exists with the correct type
    unsafe {
        &*(caches.get(&type_id).unwrap().downcast_ref::<Mutex<ReflectionCache<T>>>().unwrap() as *const Mutex<ReflectionCache<T>>)
    }
}