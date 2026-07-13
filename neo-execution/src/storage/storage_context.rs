//! StorageContext - matches C# Neo.SmartContract.StorageContext exactly

use neo_error::{CoreError, CoreResult};
use neo_vm::StackValue;
use neo_vm::{Interoperable, InteroperableError, StackItem};
use num_traits::ToPrimitive;

/// The storage context used to read and write data in smart contracts (matches C# StorageContext)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageContext {
    /// The id of the contract that owns the context
    pub id: i32,

    /// Indicates whether the context is read-only
    pub is_read_only: bool,
}

impl StorageContext {
    /// Creates a new storage context
    pub fn new(id: i32, is_read_only: bool) -> Self {
        Self { id, is_read_only }
    }

    /// Creates a read-only storage context
    pub fn read_only(id: i32) -> Self {
        Self {
            id,
            is_read_only: true,
        }
    }

    /// Creates a read-write storage context
    pub fn read_write(id: i32) -> Self {
        Self {
            id,
            is_read_only: false,
        }
    }

    /// Converts to read-only context
    pub fn as_read_only(&self) -> Self {
        Self {
            id: self.id,
            is_read_only: true,
        }
    }

    /// Encodes the storage context as bytes (id + read-only flag) matching C# serialization.
    pub fn to_bytes(&self) -> [u8; 5] {
        let mut data = [0u8; 5];
        data[..4].copy_from_slice(&self.id.to_le_bytes());
        data[4] = if self.is_read_only { 1 } else { 0 };
        data
    }

    /// Builds a storage context from encoded bytes.
    pub fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() != 5 {
            return Err(CoreError::other("StorageContext payload must be 5 bytes"));
        }

        let mut id_bytes = [0u8; 4];
        id_bytes.copy_from_slice(&bytes[..4]);
        let id = i32::from_le_bytes(id_bytes);
        let is_read_only = match bytes[4] {
            0 => false,
            1 => true,
            _ => return Err(CoreError::other("Invalid StorageContext read-only flag")),
        };

        Ok(Self { id, is_read_only })
    }

    /// Converts the context to a stack item representation used on the VM stack.
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::from_byte_string(self.to_bytes().to_vec())
    }

    /// Parses a stack item into a storage context.
    pub fn from_stack_item(item: &StackItem) -> CoreResult<Self> {
        match item {
            StackItem::ByteString(bytes) => Self::from_bytes(bytes),
            StackItem::Buffer(buffer) => Self::from_bytes(&buffer.data()),
            StackItem::Struct(items) => Self::from_stack_parts(&items.items()),
            StackItem::Array(items) => Self::from_stack_parts(&items.items()),
            _ => Err(CoreError::other(format!(
                "StorageContext stack representation must be a byte array or interop context, got {:?}",
                item.stack_item_type()
            ))),
        }
    }

    fn from_stack_parts(items: &[StackItem]) -> CoreResult<Self> {
        if items.is_empty() || items.len() > 2 {
            return Err(CoreError::other(
                "StorageContext stack representation must contain id and optional read-only flag",
            ));
        }

        let id_bigint = items[0]
            .as_int()
            .map_err(|_| CoreError::other("StorageContext id must be integer"))?;
        let id = id_bigint
            .to_i32()
            .ok_or_else(|| CoreError::other("StorageContext id out of i32 range"))?;

        let is_read_only = if items.len() == 2 {
            items[1]
                .as_bool()
                .map_err(|_| CoreError::other("StorageContext read-only flag must be boolean"))?
        } else {
            false
        };

        Ok(Self { id, is_read_only })
    }
}

impl Interoperable for StorageContext {
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), InteroperableError> {
        let bytes = value.to_byte_string_bytes().ok_or_else(|| {
            InteroperableError::InvalidType("StorageContext must be bytes".to_string())
        })?;
        let ctx = Self::from_bytes(&bytes).map_err(|error| {
            InteroperableError::InvalidData(format!("Invalid StorageContext: {error}"))
        })?;
        *self = ctx;
        Ok(())
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(StackValue::ByteString(self.to_bytes().to_vec()))
    }
}
