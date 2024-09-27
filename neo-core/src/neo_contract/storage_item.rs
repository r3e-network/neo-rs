use std::any::Any;
use std::convert::TryFrom;
use std::io::{Read, Write};
use NeoRust::types::StackItem;
use num_bigint::BigInt;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use crate::io::iserializable::ISerializable;
use crate::neo_contract::binary_serializer::BinarySerializer;
use crate::neo_contract::iinteroperable::IInteroperable;

/// Represents the values in contract storage.
pub struct StorageItem {
    value: Option<Vec<u8>>,
    cache: Option<StorageCache>,
}

enum StorageCache {
    BigInt(BigInt),
    Interoperable(Box<dyn IInteroperable<Error=()>>),
}

impl StorageItem {
    pub fn new() -> Self {
        StorageItem {
            value: None,
            cache: None,
        }
    }

    pub fn from_bytes(value: Vec<u8>) -> Self {
        StorageItem {
            value: Some(value),
            cache: None,
        }
    }

    pub fn from_bigint(value: BigInt) -> Self {
        StorageItem {
            value: None,
            cache: Some(StorageCache::BigInt(value)),
        }
    }

    pub fn from_interoperable<T: Interoperable + 'static>(interoperable: T) -> Self {
        StorageItem {
            value: None,
            cache: Some(StorageCache::Interoperable(Box::new(interoperable))),
        }
    }

    pub fn value(&self) -> Vec<u8> {
        match &self.value {
            Some(v) => v.clone(),
            None => match &self.cache {
                Some(StorageCache::BigInt(bi)) => bi.to_bytes_be().1,
                Some(StorageCache::Interoperable(interoperable)) => {
                    let stack_item = interoperable.to_stack_item(None);
                    BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
                }
                None => Vec::new(),
            },
        }
    }

    pub fn set_value(&mut self, value: Vec<u8>) {
        self.value = Some(value);
        self.cache = None;
    }

    pub fn add(&mut self, integer: &BigInt) {
        let current = BigInt::from_bytes_be(num_bigint::Sign::Plus, &self.value());
        self.set_bigint(&(current + integer));
    }

    pub fn clone(&self) -> Self {
        StorageItem {
            value: self.value.clone(),
            cache: match &self.cache {
                Some(StorageCache::BigInt(bi)) => Some(StorageCache::BigInt(bi.clone())),
                Some(StorageCache::Interoperable(interoperable)) => {
                    Some(StorageCache::Interoperable(interoperable.clone_box()))
                }
                None => None,
            },
        }
    }

    pub fn from_replica(&mut self, replica: &StorageItem) {
        self.value = replica.value.clone();
        self.cache = match &replica.cache {
            Some(StorageCache::BigInt(bi)) => Some(StorageCache::BigInt(bi.clone())),
            Some(StorageCache::Interoperable(interoperable)) => {
                Some(StorageCache::Interoperable(interoperable.clone_box()))
            }
            None => None,
        };
    }

    pub fn get_interoperable<T: Interoperable + Default>(&mut self) -> T {
        if self.cache.is_none() {
            let mut interoperable = T::default();
            let stack_item = BinarySerializer::deserialize(&self.value(), &ExecutionEngineLimits::default());
            interoperable.from_stack_item(&stack_item);
            self.cache = Some(StorageCache::Interoperable(Box::new(interoperable)));
        }
        self.value = None;
        match &self.cache {
            Some(StorageCache::Interoperable(i)) => i.as_any().downcast_ref::<T>().unwrap().clone(),
            _ => panic!("Invalid cache type"),
        }
    }

    pub fn set_bigint(&mut self, integer: &BigInt) {
        self.cache = Some(StorageCache::BigInt(integer.clone()));
        self.value = None;
    }

    pub fn set_interoperable<T: Interoperable + 'static>(&mut self, interoperable: T) {
        self.cache = Some(StorageCache::Interoperable(Box::new(interoperable)));
        self.value = None;
    }
}

impl ISerializable for StorageItem {
    fn size(&self) -> usize {
        self.value().len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_all(&self.value())?;
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;
        Ok(StorageItem::from_bytes(buffer))
    }
}

impl From<BigInt> for StorageItem {
    fn from(value: BigInt) -> Self {
        StorageItem::from_bigint(value)
    }
}

impl From<Vec<u8>> for StorageItem {
    fn from(value: Vec<u8>) -> Self {
        StorageItem::from_bytes(value)
    }
}

impl TryFrom<StorageItem> for BigInt {
    type Error = &'static str;

    fn try_from(item: StorageItem) -> Result<Self, Self::Error> {
        match item.cache {
            Some(StorageCache::BigInt(bi)) => Ok(bi),
            _ => Ok(BigInt::from_bytes_be(num_bigint::Sign::Plus, &item.value())),
        }
    }
}

trait Interoperable: Any {
    fn to_stack_item(&self) -> StackItem;
    fn from_stack_item(&mut self, item: &StackItem);
    fn clone_box(&self) -> Box<dyn IInteroperable>;
    fn as_any(&self) -> &dyn Any;
}
