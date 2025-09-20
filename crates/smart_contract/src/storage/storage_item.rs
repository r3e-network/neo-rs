//! Storage item implementation for smart contract storage.

use crate::{Error, Result};
use neo_io::Serializable;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a value in the smart contract storage system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StorageItem {
    /// The value data.
    pub value: Vec<u8>,

    /// Whether this item is constant (read-only).
    pub is_constant: bool,
}

impl StorageItem {
    /// Creates a new storage item.
    pub fn new(value: Vec<u8>, is_constant: bool) -> Self {
        Self { value, is_constant }
    }

    /// Creates a new constant storage item.
    pub fn new_constant(value: Vec<u8>) -> Self {
        Self {
            value,
            is_constant: true,
        }
    }

    /// Creates a storage item from a string.
    pub fn from_string(value: &str) -> Self {
        Self::new(value.as_bytes().to_vec(), false)
    }

    /// Creates a storage item from an integer.
    pub fn from_int(value: i32) -> Self {
        Self::new(value.to_le_bytes().to_vec(), false)
    }

    /// Creates a storage item from a boolean.
    pub fn from_bool(value: bool) -> Self {
        Self::new(vec![if value { 1 } else { 0 }], false)
    }

    /// Gets the size of the storage item in bytes.
    pub fn size(&self) -> usize {
        4 + // value length
        self.value.len() + // value data
        1 // is_constant flag
    }

    /// Converts the value to a hex string.
    pub fn to_hex_string(&self) -> String {
        hex::encode(&self.value)
    }

    /// Creates a storage item from a hex string.
    pub fn from_hex_string(hex: &str) -> Result<Self> {
        let value = hex::decode(hex)
            .map_err(|e| Error::StorageError(format!("Invalid hex string: {}", e)))?;
        Ok(Self::new(value, false))
    }

    /// Gets the value as a string if it's valid UTF-8.
    pub fn as_string(&self) -> Option<String> {
        String::from_utf8(self.value.clone()).ok()
    }

    /// Gets the value as an integer if it's 4 bytes.
    pub fn as_int(&self) -> Option<i32> {
        if self.value.len() == 4 {
            Some(i32::from_le_bytes([
                self.value[0],
                self.value[1],
                self.value[2],
                self.value[3],
            ]))
        } else {
            None
        }
    }

    /// Gets the value as a boolean if it's 1 byte.
    pub fn as_bool(&self) -> Option<bool> {
        if self.value.len() == 1 {
            Some(self.value[0] != 0)
        } else {
            None
        }
    }

    /// Checks if the storage item is empty.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Gets the length of the value.
    pub fn len(&self) -> usize {
        self.value.len()
    }

    /// Sets the value of the storage item.
    pub fn set_value(&mut self, value: Vec<u8>) -> Result<()> {
        if self.is_constant {
            return Err(Error::StorageError(
                "Cannot modify constant storage item".to_string(),
            ));
        }
        self.value = value;
        Ok(())
    }

    /// Appends data to the storage item value.
    pub fn append(&mut self, data: &[u8]) -> Result<()> {
        if self.is_constant {
            return Err(Error::StorageError(
                "Cannot modify constant storage item".to_string(),
            ));
        }
        self.value.extend_from_slice(data);
        Ok(())
    }

    /// Clears the storage item value.
    pub fn clear(&mut self) -> Result<()> {
        if self.is_constant {
            return Err(Error::StorageError(
                "Cannot modify constant storage item".to_string(),
            ));
        }
        self.value.clear();
        Ok(())
    }

    /// Creates a clone of this item as a constant.
    pub fn as_constant(&self) -> Self {
        Self {
            value: self.value.clone(),
            is_constant: true,
        }
    }
}

impl fmt::Display for StorageItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Ok(s) = String::from_utf8(self.value.clone()) {
            if s.chars().all(|c| c.is_ascii() && !c.is_control()) {
                return write!(f, "\"{}\"", s);
            }
        }

        // Otherwise, display as hex
        write!(f, "{}", self.to_hex_string())
    }
}

impl Serializable for StorageItem {
    fn size(&self) -> usize {
        // Calculate the size of the serialized StorageItem
        // This matches C# Neo's StorageItem.Size property exactly
        1 + // is_constant (bool)
        4 + // value length prefix
        self.value.len() // value bytes
    }

    fn serialize(&self, writer: &mut neo_io::BinaryWriter) -> neo_io::Result<()> {
        writer.write_bool(self.is_constant)?;
        writer.write_var_bytes(&self.value)?;
        Ok(())
    }

    fn deserialize(reader: &mut neo_io::MemoryReader) -> neo_io::Result<Self> {
        let is_constant = reader.read_boolean()?;
        let value = reader.read_var_bytes(u16::MAX as usize)?; // Max u16::MAX bytes for storage value

        Ok(StorageItem { value, is_constant })
    }
}

impl From<Vec<u8>> for StorageItem {
    fn from(value: Vec<u8>) -> Self {
        Self::new(value, false)
    }
}

impl From<&str> for StorageItem {
    fn from(value: &str) -> Self {
        Self::from_string(value)
    }
}

impl From<String> for StorageItem {
    fn from(value: String) -> Self {
        Self::from_string(&value)
    }
}

impl From<i32> for StorageItem {
    fn from(value: i32) -> Self {
        Self::from_int(value)
    }
}

impl From<bool> for StorageItem {
    fn from(value: bool) -> Self {
        Self::from_bool(value)
    }
}

impl AsRef<[u8]> for StorageItem {
    fn as_ref(&self) -> &[u8] {
        &self.value
    }
}
