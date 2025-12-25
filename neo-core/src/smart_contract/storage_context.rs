//! StorageContext - matches C# Neo.SmartContract.StorageContext exactly

use neo_vm::StackItem;

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
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 5 {
            return Err("StorageContext payload must be 5 bytes".to_string());
        }

        let mut id_bytes = [0u8; 4];
        id_bytes.copy_from_slice(&bytes[..4]);
        let id = i32::from_le_bytes(id_bytes);
        let is_read_only = match bytes[4] {
            0 => false,
            1 => true,
            _ => return Err("Invalid StorageContext read-only flag".to_string()),
        };

        Ok(Self { id, is_read_only })
    }
}

impl StorageContext {
    /// Converts the context to a stack item representation used on the VM stack.
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::from_byte_string(self.to_bytes().to_vec())
    }

    /// Parses a stack item into a storage context.
    pub fn from_stack_item(item: &StackItem) -> Result<Self, String> {
        match item {
            StackItem::ByteString(bytes) => Self::from_bytes(bytes),
            StackItem::Buffer(buffer) => {
                let data = buffer.data();
                Self::from_bytes(&data)
            }
            _ => Err("StorageContext stack representation must be a byte array".to_string()),
        }
    }
}
