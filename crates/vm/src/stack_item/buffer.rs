//! Buffer stack item implementation for the Neo Virtual Machine.

use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::stack_item_vertex::next_stack_item_id;
use crate::{VmError, VmResult};
use num_bigint::BigInt;

/// Represents a mutable byte buffer in the VM.
///
/// In C# Neo, Buffer uses reference equality (ReferenceEquals), meaning two Buffer
/// instances are only equal if they are the same instance. We achieve this in Rust
/// by assigning each Buffer a unique `id` at creation and comparing only the `id`.
#[derive(Debug)]
pub struct Buffer {
    data: Vec<u8>,
    id: usize,
}

impl PartialEq for Buffer {
    /// Compares Buffers by identity (id), matching C# ReferenceEquals semantics.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Buffer {}

impl Buffer {
    /// Creates a new buffer with the specified data.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            id: next_stack_item_id(),
        }
    }

    /// Returns the identity assigned to this buffer.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Gets the buffer data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Gets a mutable reference to the buffer data.
    pub fn data_mut(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }

    /// Returns a stable pointer to the underlying storage for identity tracking.
    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    /// Gets the type of the stack item.
    pub fn stack_item_type(&self) -> StackItemType {
        StackItemType::Buffer
    }

    /// Gets the length of the buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Gets the byte at the specified index.
    pub fn get(&self, index: usize) -> VmResult<u8> {
        self.data
            .get(index)
            .copied()
            .ok_or_else(|| VmError::invalid_operation_msg(format!("Index out of range: {index}")))
    }

    /// Sets the byte at the specified index.
    pub fn set(&mut self, index: usize, value: u8) -> VmResult<()> {
        if index >= self.data.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        self.data[index] = value;
        Ok(())
    }

    /// Converts the buffer to an integer.
    pub fn to_integer(&self) -> VmResult<BigInt> {
        if self.data.is_empty() {
            return Ok(BigInt::from(0));
        }

        let bytes = &self.data;
        let is_negative = (bytes[bytes.len() - 1] & 0x80) != 0;

        if is_negative {
            let mut magnitude_bytes = bytes.to_vec();
            let len = magnitude_bytes.len();
            magnitude_bytes[len - 1] &= 0x7F;
            let magnitude = BigInt::from_bytes_le(num_bigint::Sign::Plus, &magnitude_bytes);
            Ok(-magnitude)
        } else {
            Ok(BigInt::from_bytes_le(num_bigint::Sign::Plus, bytes))
        }
    }

    /// Converts the buffer to a boolean.
    pub fn to_boolean(&self) -> bool {
        self.data.iter().any(|&byte| byte != 0)
    }

    /// Creates a deep copy of the buffer.
    pub fn deep_copy(&self) -> Self {
        Self::new(self.data.clone())
    }

    /// Appends the given bytes to the buffer.
    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.data.extend_from_slice(slice);
    }

    /// Consumes the buffer and returns the underlying bytes.
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }
}

impl Clone for Buffer {
    fn clone(&self) -> Self {
        Self::new(self.data.clone())
    }
}

impl PartialOrd for Buffer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Buffer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.data.cmp(&other.data)
    }
}
