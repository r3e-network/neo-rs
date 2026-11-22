#![allow(clippy::mutable_key_type)]

//! Stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the stack item implementations used in the Neo VM.

use crate::ExecutionEngineLimits;
use crate::StackItemType;
use crate::error::VmError;
use crate::error::VmResult;
use crate::script::Script;
use crate::stack_item::array::Array as ArrayItem;
use crate::stack_item::buffer::Buffer as BufferItem;
use crate::stack_item::map::Map as MapItem;
use crate::stack_item::pointer::Pointer as PointerItem;
use crate::stack_item::struct_item::Struct as StructItem;
use crate::{StackValue, VmOrderedDictionary};
use num_bigint::BigInt;
use std::fmt;
use std::sync::Arc;

use super::vm_integer::VmInteger;

#[cfg(test)]
mod buffer_bytestring_equal_tests;
mod clone;
mod convert;
mod debug;
mod equality;
mod ordering;
#[cfg(test)]
#[allow(dead_code)]
mod tests;
#[cfg(test)]
mod zero_allocation_perf_tests;

/// A trait for interop interfaces that can be wrapped by a stack item.
pub trait InteropInterface: fmt::Debug + Send + Sync {
    /// Gets the type of the interop interface.
    fn interface_type(&self) -> &str;

    /// Allows downcasting to concrete types.
    fn as_any(&self) -> &dyn std::any::Any;
}

const VM_INTEGER_MAX_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum CompoundIdentity {
    Array(usize),
    Struct(usize),
    Map(usize),
}

fn compound_identity(item: &StackItem) -> Option<CompoundIdentity> {
    match item {
        StackItem::Array(array) => Some(CompoundIdentity::Array(array.id())),
        StackItem::Struct(structure) => Some(CompoundIdentity::Struct(structure.id())),
        StackItem::Map(map) => Some(CompoundIdentity::Map(map.id())),
        _ => None,
    }
}

/// Represents a value in the Neo VM.
#[derive(Clone)]
pub enum StackItem {
    /// Represents a null value.
    Null,

    /// Represents a boolean value.
    Boolean(bool),

    /// Represents an integer value.
    Integer(VmInteger),

    /// Represents an immutable byte string.
    ByteString(Vec<u8>),

    /// Represents a mutable byte buffer.
    Buffer(BufferItem),

    /// Represents an array of stack items.
    Array(ArrayItem),

    /// Represents a struct of stack items.
    Struct(StructItem),

    /// Represents a map of stack items.
    Map(MapItem),

    /// Represents a pointer to a position in a script.
    Pointer(PointerItem),

    /// Represents an interop interface.
    InteropInterface(Arc<dyn InteropInterface>),
}

/// The budget and cycle bookkeeping of [`StackItem::equals_with_limits`].
pub(super) struct LimitBudget {
    /// Remaining `MaxStackSize` units; one is spent per compared struct sub item.
    remaining_stack: u64,
    /// Remaining `MaxComparableSize` units.
    remaining_comparable: i64,
    /// Struct pairs (by id) that are currently being compared.
    visited: std::collections::HashSet<(usize, usize)>,
}

impl StackItem {
    /// The singleton True value.
    #[inline]
    #[must_use]
    pub const fn true_value() -> Self {
        Self::Boolean(true)
    }

    /// The singleton False value.
    #[inline]
    #[must_use]
    pub const fn false_value() -> Self {
        Self::Boolean(false)
    }

    /// The singleton Null value.
    #[inline]
    #[must_use]
    pub const fn null() -> Self {
        Self::Null
    }

    /// Creates a boolean stack item.
    #[inline]
    #[must_use]
    pub const fn from_bool(value: bool) -> Self {
        Self::Boolean(value)
    }

    /// Creates an integer stack item.
    #[inline]
    pub fn from_int<T: Into<BigInt>>(value: T) -> Self {
        Self::Integer(VmInteger::from_bigint(value.into()))
    }

    /// Creates an integer stack item from an i64 without heap allocation.
    #[inline]
    pub fn from_i64(value: i64) -> Self {
        Self::Integer(VmInteger::Small(value))
    }

    /// Creates a byte string stack item.
    #[inline]
    pub fn from_byte_string<T: Into<Vec<u8>>>(value: T) -> Self {
        Self::ByteString(value.into())
    }

    /// Creates a buffer stack item.
    #[inline]
    pub fn from_buffer<T: Into<Vec<u8>>>(value: T) -> Self {
        Self::Buffer(BufferItem::new(value.into()))
    }

    /// Creates an array stack item.
    #[inline]
    pub fn from_array<T: Into<Vec<Self>>>(value: T) -> Self {
        Self::Array(ArrayItem::new(value.into()))
    }

    /// Creates a struct stack item.
    #[inline]
    pub fn from_struct<T: Into<Vec<Self>>>(value: T) -> Self {
        Self::Struct(StructItem::new(value.into()))
    }

    /// Creates a map stack item.
    #[inline]
    pub fn from_map<T: Into<VmOrderedDictionary<Self, Self>>>(value: T) -> Self {
        Self::Map(MapItem::new(value))
    }

    /// Creates a pointer stack item.
    #[inline]
    #[must_use]
    pub fn from_pointer(script: Arc<Script>, position: usize) -> Self {
        Self::Pointer(PointerItem::new(script, position))
    }

    /// Creates an interop interface stack item.
    #[inline]
    pub fn from_interface<T: InteropInterface + 'static>(value: T) -> Self {
        Self::InteropInterface(Arc::new(value))
    }

    /// Returns the type of the stack item.
    #[inline]
    #[must_use]
    pub const fn stack_item_type(&self) -> StackItemType {
        match self {
            Self::Null => StackItemType::Any,
            Self::Boolean(_) => StackItemType::Boolean,
            Self::Integer(_) => StackItemType::Integer,
            Self::ByteString(_) => StackItemType::ByteString,
            Self::Buffer(_) => StackItemType::Buffer,
            Self::Array(_) => StackItemType::Array,
            Self::Struct(_) => StackItemType::Struct,
            Self::Map(_) => StackItemType::Map,
            Self::Pointer(_) => StackItemType::Pointer,
            Self::InteropInterface(_) => StackItemType::InteropInterface,
        }
    }

    /// Returns true if the stack item is null.
    #[inline]
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Converts the stack item to a boolean.
    /// PERF: Zero-allocation for most cases - avoids ByteString clone by using
    /// direct byte inspection. Only validates size without materializing StackValue.
    #[inline]
    pub fn as_bool(&self) -> VmResult<bool> {
        match self {
            Self::Null => Ok(false),
            Self::Boolean(b) => Ok(*b),
            Self::Integer(i) => {
                let value = match i.to_i64() {
                    Some(value) => StackValue::Integer(value),
                    None => StackValue::BigInteger(i.to_bigint().to_signed_bytes_le()),
                };
                Ok(crate::semantics::comparison::boolean_value(&value))
            }
            Self::ByteString(b) => {
                if b.len() > VM_INTEGER_MAX_SIZE {
                    return Err(VmError::invalid_type_simple(
                        "Cannot convert ByteString to Boolean",
                    ));
                }
                Ok(crate::semantics::comparison::boolean_value(
                    &StackValue::ByteString(b.clone()),
                ))
            }
            Self::Buffer(_b) => Ok(true),
            Self::Array(_a) => Ok(true),
            Self::Struct(_s) => Ok(true),
            Self::Map(_m) => Ok(true),
            Self::Pointer(_pointer) => Ok(true),
            Self::InteropInterface(_i) => Ok(true),
        }
    }

    /// Converts the stack item to an integer (borrowing).
    /// PERF: For Integer variant, clones the underlying BigInt. Prefer `into_int()`
    /// when consuming owned StackItem to avoid clone for VmInteger::Small.
    #[inline]
    pub fn as_int(&self) -> VmResult<BigInt> {
        match self {
            Self::Null => Err(VmError::invalid_type_simple(
                "Cannot convert Null to Integer",
            )),
            // PERF: Direct conversion without intermediate allocations
            Self::Boolean(b) => Ok(BigInt::from(i32::from(*b))),
            Self::Integer(i) => Ok(i.to_bigint()),
            Self::ByteString(b) => Self::bytestring_to_bigint(b),
            Self::Buffer(buf) => {
                if buf.len() > VM_INTEGER_MAX_SIZE {
                    return Err(VmError::invalid_type_simple(
                        "Cannot convert Buffer to Integer",
                    ));
                }
                if buf.is_empty() {
                    return Ok(BigInt::from(0));
                }
                buf.with_data(|data| Ok(Self::bytes_to_bigint(data)))
            }
            _ => Err(VmError::invalid_type_simple("Cannot convert to Integer")),
        }
    }

    /// Consuming version of `as_int` — moves the BigInt out of an Integer
    /// variant instead of cloning. Use when the StackItem is already owned
    /// (e.g., after `pop()`).
    /// PERF: Zero-allocation for VmInteger::Small (i64), avoids heap clone.
    #[inline]
    pub fn into_int(self) -> VmResult<BigInt> {
        match self {
            Self::Null => Err(VmError::invalid_type_simple(
                "Cannot convert Null to Integer",
            )),
            // PERF: Direct conversion without intermediate allocations
            Self::Boolean(b) => Ok(BigInt::from(i32::from(b))),
            Self::Integer(i) => Ok(i.into_bigint()), // PERF: MOVE — no clone for Small!
            Self::ByteString(b) => Self::bytestring_to_bigint(&b),
            Self::Buffer(buf) => {
                if buf.len() > VM_INTEGER_MAX_SIZE {
                    return Err(VmError::invalid_type_simple(
                        "Cannot convert Buffer to Integer",
                    ));
                }
                if buf.is_empty() {
                    return Ok(BigInt::from(0));
                }
                Ok(buf.with_data(Self::bytes_to_bigint))
            }
            _ => Err(VmError::invalid_type_simple("Cannot convert to Integer")),
        }
    }

    /// Shared helper: convert byte slice to BigInt with sign handling.
    fn bytes_to_bigint(data: &[u8]) -> BigInt {
        let is_negative = (data[data.len() - 1] & 0x80) != 0;
        if is_negative {
            let mut bytes_copy = data.to_vec();
            let len = bytes_copy.len();
            bytes_copy[len - 1] &= 0x7F;
            let positive_value = BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes_copy);
            let sign_bit_value = BigInt::from(1) << (len * 8 - 1);
            -(sign_bit_value - positive_value)
        } else {
            BigInt::from_bytes_le(num_bigint::Sign::Plus, data)
        }
    }

    /// Shared helper: convert ByteString (Vec<u8>) to BigInt.
    fn bytestring_to_bigint(b: &[u8]) -> VmResult<BigInt> {
        if b.len() > VM_INTEGER_MAX_SIZE {
            return Err(VmError::invalid_type_simple(
                "Cannot convert ByteString to Integer",
            ));
        }
        if b.is_empty() {
            return Ok(BigInt::from(0));
        }
        Ok(Self::bytes_to_bigint(b))
    }

    /// Returns the boolean value represented by the stack item.
    #[inline]
    pub fn as_boolean(&self) -> VmResult<bool> {
        self.as_bool()
    }

    /// Returns the integer value represented by the stack item.
    #[inline]
    pub fn as_integer(&self) -> VmResult<BigInt> {
        self.as_int()
    }

    /// Returns the pointer represented by the stack item.
    pub fn get_pointer(&self) -> VmResult<PointerItem> {
        match self {
            Self::Pointer(pointer) => Ok(pointer.clone()),
            _ => Err(VmError::invalid_type_simple(
                "Cannot convert stack item to pointer",
            )),
        }
    }

    /// Converts the stack item to a byte array.
    /// PERF: Allocates new Vec for most conversions. For owned StackItem,
    /// prefer `into_bytes()` to move ByteString without cloning.
    #[inline]
    pub fn as_bytes(&self) -> VmResult<Vec<u8>> {
        let value = match self {
            Self::Null => return Ok(Vec::new()),
            Self::Boolean(b) => StackValue::Boolean(*b),
            Self::Integer(i) => match i.to_i64() {
                Some(value) => StackValue::Integer(value),
                None => StackValue::BigInteger(i.to_bigint().to_signed_bytes_le()),
            },
            Self::ByteString(b) => StackValue::ByteString(b.clone()),
            Self::Buffer(buf) => StackValue::Buffer(buf.data()),
            _ => return Err(VmError::invalid_type_simple("Cannot convert to ByteArray")),
        };
        value
            .to_byte_string_bytes()
            .ok_or_else(|| VmError::invalid_type_simple("Cannot convert to ByteArray"))
    }

    /// Consuming version of `as_bytes` — moves the Vec out of ByteString
    /// instead of cloning. Use when the StackItem is already owned (e.g., after `pop()`).
    /// PERF: Zero-allocation for ByteString variant (moves Vec instead of cloning).
    #[inline]
    pub fn into_bytes(self) -> VmResult<Vec<u8>> {
        let value = match self {
            Self::Null => return Ok(Vec::new()),
            Self::Boolean(b) => StackValue::Boolean(b),
            Self::Integer(i) => match i.to_i64() {
                Some(value) => StackValue::Integer(value),
                None => StackValue::BigInteger(i.into_bigint().to_signed_bytes_le()),
            },
            Self::ByteString(b) => StackValue::ByteString(b),
            Self::Buffer(buf) => StackValue::Buffer(buf.data()),
            _ => return Err(VmError::invalid_type_simple("Cannot convert to ByteArray")),
        };
        value
            .to_byte_string_bytes()
            .ok_or_else(|| VmError::invalid_type_simple("Cannot convert to ByteArray"))
    }

    /// Returns a borrowed byte slice for variants that own contiguous bytes
    /// (`ByteString`). For other convertible variants the caller should fall
    /// back to [`as_bytes`](Self::as_bytes).
    ///
    /// This avoids the `Vec` allocation that `as_bytes()` performs, which is
    /// significant in hot paths like map key validation.
    #[inline]
    pub fn as_bytes_ref(&self) -> Option<&[u8]> {
        match self {
            Self::ByteString(b) => Some(b.as_slice()),
            _ => None,
        }
    }

    /// Converts the stack item to an array.
    pub fn as_array(&self) -> VmResult<Vec<Self>> {
        match self {
            Self::Array(a) => Ok(a.items()),
            Self::Struct(s) => Ok(s.items()),
            _ => Err(VmError::invalid_type_simple("Cannot convert to Array")),
        }
    }

    /// Converts the stack item to a map.
    pub fn as_map(&self) -> VmResult<VmOrderedDictionary<Self, Self>> {
        match self {
            Self::Map(m) => Ok(m.items()),
            _ => Err(VmError::invalid_type_simple("Cannot convert to Map")),
        }
    }

    /// Gets the interop interface from the stack item.
    /// Production implementation with proper type downcasting for C# compatibility.
    pub fn as_interface<T: InteropInterface + 'static>(&self) -> VmResult<&T> {
        match self {
            Self::InteropInterface(i) => {
                // Use Any trait for runtime type checking (matches C# reflection pattern)
                let interface_any = i.as_any();

                // Attempt to downcast to the requested type
                interface_any.downcast_ref::<T>().ok_or_else(|| {
                    VmError::invalid_type_simple(format!(
                        "Cannot cast InteropInterface to type {}",
                        std::any::type_name::<T>()
                    ))
                })
            }
            _ => Err(VmError::invalid_type_simple(
                "Stack item is not an InteropInterface",
            )),
        }
    }

    /// Converts the stack item to the specified type.
    /// PERF: Zero-allocation for same-type conversions - returns borrowed clone.
    /// For different types, conversion may allocate as required by protocol.
    pub fn convert_to(&self, item_type: StackItemType) -> VmResult<Self> {
        // PERF: Fast path - same type, just clone (cheap for primitives)
        if self.stack_item_type() == item_type {
            return Ok(self.clone());
        }

        match (self, item_type) {
            // PROTOCOL: Null converts to Boolean as Null (not Boolean(false))
            (Self::Null, StackItemType::Boolean) => {
                return Ok(Self::Null);
            }
            (item, target_type @ StackItemType::Boolean) => {
                if let Self::ByteString(bytes) = item
                    && bytes.len() > VM_INTEGER_MAX_SIZE
                {
                    return Err(VmError::invalid_type_simple(
                        "Cannot convert ByteString to Boolean",
                    ));
                }
                let _ = target_type;
                return Ok(Self::Boolean(item.as_bool()?));
            }
            (
                item
                @ (Self::Boolean(_) | Self::Integer(_) | Self::ByteString(_) | Self::Buffer(_)),
                target_type @ (StackItemType::ByteString | StackItemType::Buffer),
            ) => {
                // Reuse the shared ABI conversion rules instead of a local
                // byte-conversion table. `to_byte_string_bytes` handles
                // integer sign encoding and Boolean/Null coercion.
                let value = StackValue::try_from(item.clone()).map_err(|_| {
                    VmError::invalid_type_simple(format!("Cannot convert to {target_type:?}"))
                })?;
                let converted =
                    crate::semantics::conversion::convert_value(value, target_type.to_byte())
                        .map_err(VmError::invalid_operation_msg)?;
                return StackItem::try_from(converted)
                    .map_err(|_| VmError::invalid_type_simple("Invalid converted stack item"));
            }
            (Self::Null, StackItemType::ByteString) => {
                return Ok(Self::ByteString(Vec::new()));
            }
            (Self::Null, StackItemType::Buffer) => {
                return Ok(Self::Buffer(BufferItem::new(Vec::new())));
            }
            _ => {}
        }

        match item_type {
            StackItemType::Boolean => Ok(Self::Boolean(self.as_bool()?)),
            StackItemType::Integer => Ok(Self::Integer(VmInteger::from_bigint(self.as_int()?))),
            _ => Err(VmError::invalid_type_simple(format!(
                "Cannot convert to {item_type:?}"
            ))),
        }
    }
}
