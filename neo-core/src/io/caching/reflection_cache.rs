use std::any::Any;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::marker::PhantomData;
use crate::io::memory_reader::MemoryReader;
use crate::io::serializable_trait::SerializableTrait;

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

//
// // Usage Example:
//
// // Define an enum to use as keys.
// #[derive(Copy, Clone, Eq, PartialEq, Hash)]
// pub enum MyEnum {
//     TypeA,
//     TypeB,
// }
//
// // Define some structs to associate with the enum variants.
// #[derive(Default)]
// pub struct TypeA {
//     // fields...
// }
//
// #[derive(Default)]
// pub struct TypeB {
//     // fields...
// }
//
// // Initialize the reflection cache with the types.
// static REFLECTION_CACHE: OnceLock<ReflectionCache<MyEnum>> = OnceLock::new();
//
// fn initialize_cache() {
//     let mut cache = ReflectionCache::new();
//     cache.register::<TypeA>(MyEnum::TypeA);
//     cache.register::<TypeB>(MyEnum::TypeB);
//     REFLECTION_CACHE.set(cache).unwrap();
// }
//
// fn main() {
//     initialize_cache();
//
//     // Create an instance of TypeA.
//     if let Some(instance) = REFLECTION_CACHE.get().unwrap().create_instance(MyEnum::TypeA) {
//         // Downcast the boxed Any to the specific type.
//         if let Ok(type_a) = instance.downcast::<TypeA>() {
//             // Use the instance...
//             println!("Created an instance of TypeA!");
//         }
//     }
//
//     // Create an instance of TypeB.
//     if let Some(instance) = REFLECTION_CACHE.get().unwrap().create_instance(MyEnum::TypeB) {
//         if let Ok(type_b) = instance.downcast::<TypeB>() {
//             println!("Created an instance of TypeB!");
//         }
//     }
// }
