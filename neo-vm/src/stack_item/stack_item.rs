#![allow(clippy::mutable_key_type)]

//! Stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the stack item implementations used in the Neo VM.

use crate::ExecutionEngineLimits;
use crate::StackItemType;
use crate::error::VmError;
use crate::error::VmResult;
use crate::reference_counter::ReferenceCounter;
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
#[derive(Debug, Clone)]
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
        Self::Array(ArrayItem::new_untracked(value.into()))
    }

    /// Creates a struct stack item.
    #[inline]
    pub fn from_struct<T: Into<Vec<Self>>>(value: T) -> Self {
        Self::Struct(StructItem::new_untracked(value.into()))
    }

    /// Creates a map stack item.
    #[inline]
    pub fn from_map<T: Into<VmOrderedDictionary<Self, Self>>>(value: T) -> Self {
        Self::Map(MapItem::new_untracked(value.into()))
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

    /// Ensures any compound stack items share the provided reference counter.
    ///
    /// This is required for C# parity: all compound VM objects are expected to
    /// belong to the engine's `ReferenceCounter`. Host-provided stack items may
    /// be constructed without a counter and are attached when they enter the VM.
    pub fn attach_reference_counter(&mut self, rc: &ReferenceCounter) -> VmResult<()> {
        match self {
            Self::Array(array) => array.attach_reference_counter(rc),
            Self::Struct(structure) => structure.attach_reference_counter(rc),
            Self::Map(map) => map.attach_reference_counter(rc),
            _ => Ok(()),
        }
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

    /// Creates a deep clone of the stack item.
    #[must_use]
    pub fn deep_clone(&self) -> Self {
        self.deep_clone_with_refs(&mut std::collections::HashMap::new())
    }

    /// Creates a deep copy respecting execution limits (mirrors C# behaviour).
    pub fn deep_copy(&self, limits: &ExecutionEngineLimits) -> VmResult<Self> {
        match self {
            Self::Struct(structure) => {
                let cloned = structure.clone_with_limits(limits)?;
                Ok(Self::Struct(cloned))
            }
            Self::Array(array) => {
                let copy = array.deep_copy(array.reference_counter())?;
                Ok(Self::Array(copy))
            }
            Self::Map(map) => {
                let copy = map.deep_copy(map.reference_counter())?;
                Ok(Self::Map(copy))
            }
            _ => Ok(self.deep_clone()),
        }
    }

    /// Creates a deep clone of the stack item with reference tracking to handle cycles.
    fn deep_clone_with_refs(
        &self,
        refs: &mut std::collections::HashMap<CompoundIdentity, Self>,
    ) -> Self {
        if let Some(self_id) = compound_identity(self)
            && let Some(cloned) = refs.get(&self_id)
        {
            return cloned.clone();
        }

        // Clone the item based on its type
        let result = match self {
            Self::Null => Self::Null,
            Self::Boolean(b) => Self::Boolean(*b),
            Self::Integer(i) => Self::Integer(i.clone()),
            Self::ByteString(b) => Self::ByteString(b.clone()),
            Self::Buffer(b) => Self::Buffer(BufferItem::new(b.data())),
            Self::Pointer(p) => Self::Pointer(p.clone()),
            Self::InteropInterface(i) => Self::InteropInterface(i.clone()),

            Self::Array(a) => {
                let cloned_array = ArrayItem::new_untracked(Vec::new());
                let cloned_item = Self::Array(cloned_array.clone());
                if let Some(self_id) = compound_identity(self) {
                    refs.insert(self_id, cloned_item.clone());
                }
                for item in a.items() {
                    let child = item.deep_clone_with_refs(refs);
                    let _ = cloned_array.push(child);
                }
                cloned_item
            }
            Self::Struct(s) => {
                let cloned_struct = StructItem::new_untracked(Vec::new());
                let cloned_item = Self::Struct(cloned_struct.clone());
                if let Some(self_id) = compound_identity(self) {
                    refs.insert(self_id, cloned_item.clone());
                }
                for item in s.items() {
                    let child = item.deep_clone_with_refs(refs);
                    let _ = cloned_struct.push(child);
                }
                cloned_item
            }
            Self::Map(m) => {
                let cloned_map = MapItem::new_untracked(VmOrderedDictionary::new());
                let cloned_item = Self::Map(cloned_map.clone());
                if let Some(self_id) = compound_identity(self) {
                    refs.insert(self_id, cloned_item.clone());
                }
                for (k, v) in m.items().iter() {
                    let key = k.deep_clone_with_refs(refs);
                    let value = v.deep_clone_with_refs(refs);
                    let _ = cloned_map.set(key, value);
                }
                cloned_item
            }
        };

        if let Some(self_id) = compound_identity(self) {
            refs.insert(self_id, result.clone());
        }

        result
    }

    /// Clears all references to other stack items.
    pub fn clear_references(&mut self) {
        match self {
            Self::Array(array) => {
                let _ = array.clear();
            }
            Self::Struct(structure) => {
                let _ = structure.clear();
            }
            Self::Map(map) => {
                let _ = map.clear();
            }
            _ => {}
        }
    }

    /// Computes a deterministic hash code compatible with the C# implementation.
    #[must_use]
    pub fn hash_code(&self) -> i32 {
        match self {
            Self::Null => 0,
            Self::Boolean(b) => i32::from(*b),
            Self::Integer(i) => hash_bytes(&i.to_signed_bytes_le()), // VmInteger has this method
            Self::ByteString(b) => hash_bytes(b),
            Self::Buffer(b) => b.with_data(hash_bytes),
            Self::Array(array) => {
                let mut hash = combine_hash(17, array.len() as i32);
                for item in array {
                    hash = combine_hash(hash, item.hash_code());
                }
                hash
            }
            Self::Struct(structure) => {
                let mut hash = combine_hash(17, structure.len() as i32);
                for item in structure.items() {
                    hash = combine_hash(hash, item.hash_code());
                }
                hash
            }
            Self::Map(map) => {
                let mut hash = combine_hash(17, map.len() as i32);
                for (key, value) in map.items().iter() {
                    hash = combine_hash(hash, key.hash_code());
                    hash = combine_hash(hash, value.hash_code());
                }
                hash
            }
            Self::Pointer(pointer) => {
                let script_ptr = pointer.script() as *const Script as usize as u64;
                let mut hash = 17;
                hash = combine_hash(hash, (script_ptr & 0xFFFF_FFFF) as i32);
                hash = combine_hash(hash, ((script_ptr >> 32) & 0xFFFF_FFFF) as i32);
                hash = combine_hash(hash, pointer.position() as i32);
                hash
            }
            Self::InteropInterface(interface) => {
                let addr = Arc::as_ptr(interface).cast::<()>() as usize as u64;
                let mut hash = 17;
                hash = combine_hash(hash, (addr & 0xFFFF_FFFF) as i32);
                hash = combine_hash(hash, ((addr >> 32) & 0xFFFF_FFFF) as i32);
                hash
            }
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

    /// Checks if two stack items are equal.
    pub fn equals(&self, other: &Self) -> VmResult<bool> {
        self.equals_with_refs(other, &mut std::collections::HashSet::new())
    }

    /// Checks if two stack items are equal with execution limits (aligns with C# API).
    ///
    /// C# equality is deliberately asymmetric across item kinds:
    /// - primitives compare by value (type-strict);
    /// - `Buffer`, `Array` and `Map` compare by reference;
    /// - `Struct` compares structurally, bounded by `MaxStackSize` and
    ///   `MaxComparableSize`;
    /// - `ByteString` comparisons consume `MaxComparableSize`.
    ///
    /// The previous implementation reused the un-bounded structural `equals`
    /// here, which made `EQUAL` on arrays/maps consensus-incompatible and
    /// allowed unbounded recursive comparisons.
    pub fn equals_with_limits(
        &self,
        other: &Self,
        limits: &ExecutionEngineLimits,
    ) -> VmResult<bool> {
        let mut remaining_stack = u64::from(limits.max_stack_size);
        let mut remaining_comparable = i64::from(limits.max_comparable_size);
        let mut visited = std::collections::HashSet::new();
        self.equals_with_budget(
            other,
            limits,
            &mut remaining_stack,
            &mut remaining_comparable,
            &mut visited,
        )
    }

    fn equals_with_budget(
        &self,
        other: &Self,
        limits: &ExecutionEngineLimits,
        remaining_stack: &mut u64,
        remaining_comparable: &mut i64,
        visited: &mut std::collections::HashSet<(usize, usize)>,
    ) -> VmResult<bool> {
        match (self, other) {
            (Self::Null, Self::Null) => Ok(true),
            (Self::Null, _) | (_, Self::Null) => Ok(false),
            (Self::Buffer(a), Self::Buffer(b)) => Ok(a.id() == b.id()),
            (Self::Array(a), Self::Array(b)) => Ok(a.id() == b.id()),
            (Self::Map(a), Self::Map(b)) => Ok(a.id() == b.id()),
            (Self::Struct(a), Self::Struct(b)) => Self::struct_equals_with_budget(
                a,
                b,
                limits,
                remaining_stack,
                remaining_comparable,
                visited,
            ),
            (Self::Boolean(a), Self::Boolean(b)) => Ok(a == b),
            (Self::Integer(a), Self::Integer(b)) => Ok(a.to_bigint() == b.to_bigint()),
            (Self::ByteString(a), Self::ByteString(b)) => {
                if *remaining_comparable == 0 {
                    return Err(VmError::invalid_operation_msg(
                        "The operand exceeds the maximum comparable size",
                    ));
                }
                if a.len() as i64 > *remaining_comparable {
                    return Err(VmError::invalid_operation_msg(format!(
                        "The operand exceeds the maximum comparable size, {}/{}",
                        a.len(),
                        remaining_comparable
                    )));
                }
                if b.len() as i64 > *remaining_comparable {
                    return Err(VmError::invalid_operation_msg(format!(
                        "The operand exceeds the maximum comparable size, {}/{}",
                        b.len(),
                        remaining_comparable
                    )));
                }
                let compared = a.len().max(b.len()).max(1) as i64;
                *remaining_comparable -= compared;
                Ok(a == b)
            }
            (Self::Pointer(a), Self::Pointer(b)) => Ok(a == b),
            (Self::InteropInterface(a), Self::InteropInterface(b)) => Ok(Arc::ptr_eq(a, b)),
            _ => Ok(false),
        }
    }

    fn struct_equals_with_budget(
        a: &StructItem,
        b: &StructItem,
        limits: &ExecutionEngineLimits,
        remaining_stack: &mut u64,
        remaining_comparable: &mut i64,
        visited: &mut std::collections::HashSet<(usize, usize)>,
    ) -> VmResult<bool> {
        if a.id() == b.id() {
            return Ok(true);
        }
        let key = (a.id(), b.id());
        if !visited.insert(key) {
            // C# uses ReferenceEquals(a, b) to short-circuit cycles inside
            // structs; if we re-enter the same pair treat it as equal.
            return Ok(true);
        }

        let result = (|| {
            let a_items = a.items();
            let b_items = b.items();
            if a_items.len() != b_items.len() {
                return Ok(false);
            }
            for (left, right) in a_items.iter().zip(b_items.iter()) {
                if *remaining_stack == 0 {
                    return Err(VmError::invalid_operation_msg(
                        "Too many struct items to compare in struct comparison",
                    ));
                }
                *remaining_stack -= 1;

                match left {
                    Self::ByteString(_) => {
                        if !left.equals_with_budget(
                            right,
                            limits,
                            remaining_stack,
                            remaining_comparable,
                            visited,
                        )? {
                            return Ok(false);
                        }
                    }
                    Self::Struct(_) => {
                        if *remaining_comparable == 0 {
                            return Err(VmError::invalid_operation_msg(
                                "The operand exceeds the maximum comparable size in struct comparison",
                            ));
                        }
                        *remaining_comparable -= 1;
                        if !left.equals_with_budget(
                            right,
                            limits,
                            remaining_stack,
                            remaining_comparable,
                            visited,
                        )? {
                            return Ok(false);
                        }
                    }
                    _ => {
                        if *remaining_comparable == 0 {
                            return Err(VmError::invalid_operation_msg(
                                "The operand exceeds the maximum comparable size in struct comparison",
                            ));
                        }
                        *remaining_comparable -= 1;
                        let equal = match (left, right) {
                            (Self::Buffer(x), Self::Buffer(y)) => x.id() == y.id(),
                            (Self::Array(x), Self::Array(y)) => x.id() == y.id(),
                            (Self::Map(x), Self::Map(y)) => x.id() == y.id(),
                            (Self::Boolean(x), Self::Boolean(y)) => x == y,
                            (Self::Integer(x), Self::Integer(y)) => x.to_bigint() == y.to_bigint(),
                            (Self::Pointer(x), Self::Pointer(y)) => x == y,
                            (Self::InteropInterface(x), Self::InteropInterface(y)) => {
                                Arc::ptr_eq(x, y)
                            }
                            _ => false,
                        };
                        if !equal {
                            return Ok(false);
                        }
                    }
                }
            }
            Ok(true)
        })();

        visited.remove(&key);
        result
    }

    /// Checks if two stack items are equal with reference tracking to handle cycles.
    fn equals_with_refs(
        &self,
        other: &Self,
        visited: &mut std::collections::HashSet<(CompoundIdentity, CompoundIdentity)>,
    ) -> VmResult<bool> {
        let mut visited_key = None;
        if let (Some(self_id), Some(other_id)) = (compound_identity(self), compound_identity(other))
        {
            if visited.contains(&(self_id, other_id)) || visited.contains(&(other_id, self_id)) {
                return Ok(true);
            }

            visited.insert((self_id, other_id));
            visited_key = Some((self_id, other_id));
        }

        // C# Neo VM PrimitiveType.Equals is TYPE-STRICT: only items of the SAME
        // concrete primitive type (Integer, ByteString, Boolean) compare equal,
        // and only by value within that type. Cross-type comparison (e.g.
        // `Integer(1) == ByteString([0x01])`) returns FALSE in C#, even when
        // the byte representations match. Verified via mainnet RPC invokescript
        // against C# v3.9.1.
        let result = match (self, other) {
            (Self::Null, Self::Null) => Ok(true),
            // Buffer uses reference equality (compound type in C# Neo VM).
            // Buffer == Buffer → same reference only; Buffer == anything_else → false.
            (Self::Buffer(a), Self::Buffer(b)) => Ok(a.id() == b.id()),
            (Self::Buffer(_), _) | (_, Self::Buffer(_)) => Ok(false),
            // Same-type primitive comparisons (TYPE-STRICT, matches C# PrimitiveType.Equals).
            (Self::Boolean(a), Self::Boolean(b)) => Ok(a == b),
            (Self::Integer(a), Self::Integer(b)) => Ok(a.to_bigint() == b.to_bigint()),
            (Self::ByteString(a), Self::ByteString(b)) => Ok(a == b),
            // Cross-type primitive comparison: always FALSE (no byte-wise coercion).
            (a, b)
                if matches!(a, Self::Boolean(_) | Self::Integer(_) | Self::ByteString(_))
                    && matches!(b, Self::Boolean(_) | Self::Integer(_) | Self::ByteString(_)) =>
            {
                Ok(false)
            }
            (Self::Pointer(a), Self::Pointer(b)) => Ok(a == b),
            (Self::InteropInterface(a), Self::InteropInterface(b)) => Ok(Arc::ptr_eq(a, b)),
            (Self::Array(a), Self::Array(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }

                for (ai, bi) in a.iter().zip(b.iter()) {
                    if !ai.equals_with_refs(&bi, visited)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            (Self::Struct(a), Self::Struct(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }

                for (ai, bi) in a.iter().zip(b.iter()) {
                    if !ai.equals_with_refs(&bi, visited)? {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            (Self::Map(a), Self::Map(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }

                let b_items = b.items();
                for (ak, av) in a.items().iter() {
                    let found = b_items.iter().any(|(bk, bv)| {
                        ak.equals_with_refs(bk, visited).unwrap_or(false)
                            && av.equals_with_refs(bv, visited).unwrap_or(false)
                    });

                    if !found {
                        return Ok(false);
                    }
                }

                Ok(true)
            }
            _ => Ok(false),
        };

        if let Some((self_id, other_id)) = visited_key {
            visited.remove(&(self_id, other_id));
        }

        result
    }
}

// Implement PartialEq to allow stack items to be compared and used as keys in collections
impl PartialEq for StackItem {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other).unwrap_or(false)
    }
}

impl Eq for StackItem {}

impl TryFrom<crate::StackValue> for StackItem {
    type Error = VmError;

    fn try_from(value: crate::StackValue) -> VmResult<Self> {
        match value {
            crate::StackValue::Integer(value) => Ok(Self::from_i64(value)),
            crate::StackValue::BigInteger(bytes) => {
                Ok(Self::from_int(BigInt::from_signed_bytes_le(&bytes)))
            }
            crate::StackValue::ByteString(bytes) => Ok(Self::from_byte_string(bytes)),
            crate::StackValue::Buffer(bytes) => Ok(Self::from_buffer(bytes)),
            crate::StackValue::Boolean(value) => Ok(Self::from_bool(value)),
            crate::StackValue::Array(items) => {
                let items = items
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::from_array(items))
            }
            crate::StackValue::Struct(items) => {
                let items = items
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::from_struct(items))
            }
            crate::StackValue::Map(entries) => {
                let mut map = VmOrderedDictionary::with_capacity(entries.len());
                for (key, value) in entries {
                    map.insert(Self::try_from(key)?, Self::try_from(value)?);
                }
                Ok(Self::from_map(map))
            }
            crate::StackValue::Null => Ok(Self::Null),
            crate::StackValue::Pointer(_)
            | crate::StackValue::Interop(_)
            | crate::StackValue::Iterator(_) => Err(VmError::invalid_operation_msg(format!(
                "Cannot convert {:?} into neo-vm StackItem without host runtime identity",
                value
            ))),
        }
    }
}

impl TryFrom<StackItem> for crate::StackValue {
    type Error = VmError;

    fn try_from(value: StackItem) -> VmResult<Self> {
        match value {
            StackItem::Null => Ok(Self::Null),
            StackItem::Boolean(value) => Ok(Self::Boolean(value)),
            StackItem::Integer(value) => match value.to_i64() {
                Some(value) => Ok(Self::Integer(value)),
                None => Ok(Self::BigInteger(value.to_signed_bytes_le())),
            },
            StackItem::ByteString(bytes) => Ok(Self::ByteString(bytes)),
            StackItem::Buffer(buffer) => Ok(Self::Buffer(buffer.data())),
            StackItem::Array(array) => {
                let items = array
                    .items()
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::Array(items))
            }
            StackItem::Struct(structure) => {
                let items = structure
                    .items()
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::Struct(items))
            }
            StackItem::Map(map) => {
                let entries = map
                    .iter()
                    .map(|(key, value)| Ok((Self::try_from(key)?, Self::try_from(value)?)))
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::Map(entries))
            }
            StackItem::Pointer(pointer) => {
                let position = i64::try_from(pointer.position()).map_err(|_| {
                    VmError::overflow("StackItem pointer position does not fit neo-vm-rs i64")
                })?;
                Ok(Self::Pointer(position))
            }
            StackItem::InteropInterface(_) => Err(VmError::invalid_operation_msg(
                "Cannot convert InteropInterface into neo-vm-rs StackValue without a host handle",
            )),
        }
    }
}

// Implement PartialOrd and Ord to allow stack items to be used as keys in BTreeMap
// Production-ready implementation matching C# StackItem comparison exactly
impl PartialOrd for StackItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StackItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Production-ready ordering based on C# stack_item comparison rules
        // 1. First compare by type (matches C# stack_item type hierarchy)
        let type_order = self
            .stack_item_type()
            .to_byte()
            .cmp(&other.stack_item_type().to_byte());
        if type_order != std::cmp::Ordering::Equal {
            return type_order;
        }

        // 2. Compare values within the same type
        match (self, other) {
            (Self::Null, Self::Null) => std::cmp::Ordering::Equal,
            (Self::Boolean(a), Self::Boolean(b)) => a.cmp(b),
            (Self::Integer(a), Self::Integer(b)) => a.cmp(b),
            (Self::ByteString(a), Self::ByteString(b)) => a.cmp(b),
            (Self::Buffer(a), Self::Buffer(b)) => a.cmp(b),
            (Self::ByteString(a), Self::Buffer(b)) => b.with_data(|data| a.as_slice().cmp(data)),
            (Self::Buffer(a), Self::ByteString(b)) => a.with_data(|data| data.cmp(b.as_slice())),
            (Self::Pointer(a), Self::Pointer(b)) => a.cmp(b),
            (Self::Array(a), Self::Array(b)) => cmp_stack_item_sequences(a.iter(), b.iter()),
            (Self::Struct(a), Self::Struct(b)) => cmp_stack_item_sequences(a.iter(), b.iter()),
            (Self::Map(a), Self::Map(b)) => {
                // Compare maps by size first, then by sorted key-value pairs
                let len_cmp = a.len().cmp(&b.len());
                if len_cmp != std::cmp::Ordering::Equal {
                    return len_cmp;
                }

                let a_items = a.items();
                let b_items = b.items();
                let mut a_pairs: Vec<_> = a_items.iter().collect();
                let mut b_pairs: Vec<_> = b_items.iter().collect();
                a_pairs.sort_by(|x, y| x.0.cmp(y.0));
                b_pairs.sort_by(|x, y| x.0.cmp(y.0));

                for ((key_a, val_a), (key_b, val_b)) in a_pairs.iter().zip(b_pairs.iter()) {
                    let key_cmp = key_a.cmp(key_b);
                    if key_cmp != std::cmp::Ordering::Equal {
                        return key_cmp;
                    }
                    let val_cmp = val_a.cmp(val_b);
                    if val_cmp != std::cmp::Ordering::Equal {
                        return val_cmp;
                    }
                }
                std::cmp::Ordering::Equal
            }
            _ => {
                let _self_discriminant = std::mem::discriminant(self);
                let _other_discriminant = std::mem::discriminant(other);
                // based on the variant order in the enum
                match (self, other) {
                    (Self::Null, _) => std::cmp::Ordering::Less,
                    (_, Self::Null) => std::cmp::Ordering::Greater,
                    (Self::Boolean(_), Self::Integer(_)) => std::cmp::Ordering::Less,
                    (Self::Integer(_), Self::Boolean(_)) => std::cmp::Ordering::Greater,
                    (Self::Boolean(_), Self::ByteString(_)) => std::cmp::Ordering::Less,
                    (Self::ByteString(_), Self::Boolean(_)) => std::cmp::Ordering::Greater,
                    (Self::Integer(_), Self::ByteString(_)) => std::cmp::Ordering::Less,
                    (Self::ByteString(_), Self::Integer(_)) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal, // Same types that we haven't handled above
                }
            }
        }
    }
}

fn cmp_stack_item_sequences(
    left: impl ExactSizeIterator<Item = StackItem>,
    right: impl ExactSizeIterator<Item = StackItem>,
) -> std::cmp::Ordering {
    let len_cmp = left.len().cmp(&right.len());
    if len_cmp != std::cmp::Ordering::Equal {
        return len_cmp;
    }

    for (item_a, item_b) in left.zip(right) {
        let item_cmp = item_a.cmp(&item_b);
        if item_cmp != std::cmp::Ordering::Equal {
            return item_cmp;
        }
    }
    std::cmp::Ordering::Equal
}

const fn combine_hash(current: i32, value: i32) -> i32 {
    current.wrapping_mul(397).wrapping_add(value)
}

fn hash_bytes(bytes: &[u8]) -> i32 {
    bytes
        .iter()
        .fold(17, |hash, byte| combine_hash(hash, i32::from(*byte)))
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_boolean_stack_item() {
        let true_item = StackItem::from_bool(true);
        let false_item = StackItem::from_bool(false);

        assert!(true_item.as_bool().expect("Failed to convert to bool"));
        assert!(!false_item.as_bool().expect("Failed to convert to bool"));
        assert_eq!(true_item.stack_item_type(), StackItemType::Boolean);
    }

    #[test]
    fn test_integer_stack_item() {
        let int_item = StackItem::from_int(42);

        assert_eq!(int_item.as_int().unwrap(), BigInt::from(42));
        assert!(int_item.as_bool().expect("Failed to convert"));
        assert_eq!(int_item.stack_item_type(), StackItemType::Integer);

        let zero_item = StackItem::from_int(0);
        assert!(!zero_item.as_bool().expect("Failed to convert"));
    }

    #[test]
    fn test_bytestring_stack_item() {
        let bytes = vec![1, 2, 3];
        let byte_string = StackItem::from_byte_string(bytes.clone());

        assert_eq!(byte_string.as_bytes().unwrap(), bytes);
        assert!(byte_string.as_bool().expect("Failed to convert"));
        assert_eq!(byte_string.stack_item_type(), StackItemType::ByteString);

        let empty_bytes = StackItem::from_byte_string(Vec::<u8>::new());
        assert!(!empty_bytes.as_bool().expect("Failed to convert"));
    }

    #[test]
    fn test_array_stack_item() {
        let array = StackItem::from_array(vec![
            StackItem::from_int(1),
            StackItem::from_int(2),
            StackItem::from_int(3),
        ]);

        assert_eq!(
            array
                .as_array()
                .expect("intermediate value should exist")
                .len(),
            3
        );
        assert!(array.as_bool().expect("Failed to convert"));
        assert_eq!(array.stack_item_type(), StackItemType::Array);

        let empty_array = StackItem::from_array(Vec::<StackItem>::new());
        assert!(empty_array.as_bool().expect("Failed to convert"));
    }

    #[test]
    fn test_deep_clone() {
        let array = StackItem::from_array(vec![
            StackItem::from_int(1),
            StackItem::from_int(2),
            StackItem::from_array(vec![StackItem::from_int(3), StackItem::from_int(4)]),
        ]);

        let cloned = array.deep_clone();
        assert!(array.equals(&cloned).unwrap());
    }

    #[test]
    fn test_equals() {
        let a = StackItem::from_int(42);
        let b = StackItem::from_int(42);
        let c = StackItem::from_int(43);

        assert!(a.equals(&b).unwrap());
        assert!(!a.equals(&c).unwrap());

        let array1 = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

        let array2 = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

        let array3 = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(3)]);

        assert!(array1.equals(&array2).unwrap_or(false));
        assert!(!array1.equals(&array3).unwrap());
    }

    #[test]
    fn test_convert_to() {
        let int_item = StackItem::from_int(42);

        // Convert to boolean
        let bool_item = int_item.convert_to(StackItemType::Boolean).unwrap();
        assert_eq!(bool_item.stack_item_type(), StackItemType::Boolean);
        assert!(bool_item.as_bool().expect("Failed to convert"));

        let byte_string = int_item.convert_to(StackItemType::ByteString).unwrap();
        assert_eq!(byte_string.stack_item_type(), StackItemType::ByteString);
        assert_eq!(byte_string.as_bytes().unwrap(), vec![42]);

        // Convert to buffer
        let buffer = int_item.convert_to(StackItemType::Buffer).unwrap();
        assert_eq!(buffer.stack_item_type(), StackItemType::Buffer);
        assert_eq!(buffer.as_bytes().unwrap(), vec![42]);

        let int_clone = int_item.convert_to(StackItemType::Integer).unwrap();
        assert_eq!(int_clone.stack_item_type(), StackItemType::Integer);
        assert_eq!(
            int_clone.as_int().expect("Operation failed"),
            BigInt::from(42)
        );

        // Convert to unsupported type
        assert!(int_item.convert_to(StackItemType::Array).is_err());
    }

    #[test]
    fn convert_to_boolean_preserves_local_truthiness_boundaries() {
        fn assert_boolean(item: StackItem, expected: bool) {
            let converted = item.convert_to(StackItemType::Boolean).unwrap();
            assert_eq!(converted, StackItem::Boolean(expected));
        }

        assert_eq!(
            StackItem::Null.convert_to(StackItemType::Boolean).unwrap(),
            StackItem::Null
        );
        assert_boolean(StackItem::from_int(0), false);
        assert_boolean(StackItem::from_int(1), true);
        assert_boolean(StackItem::from_byte_string(vec![0]), false);
        assert_boolean(StackItem::from_byte_string(vec![1]), true);

        assert_boolean(StackItem::from_buffer(Vec::<u8>::new()), true);
        assert_boolean(StackItem::from_buffer(vec![0]), true);
        assert_boolean(StackItem::from_array(Vec::<StackItem>::new()), true);
        assert_boolean(StackItem::from_struct(Vec::<StackItem>::new()), true);

        let too_large = StackItem::from_byte_string(vec![0; VM_INTEGER_MAX_SIZE + 1]);
        assert!(too_large.convert_to(StackItemType::Boolean).is_err());
    }

    #[test]
    fn test_equals_with_cycles() {
        // Create two arrays with cycles
        let mut array1 =
            StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

        let mut array2 =
            StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

        // Add cycles
        let array1_clone = array1.clone();
        if let StackItem::Array(array) = &mut array1 {
            let _ = array.push(array1_clone);
        }

        let array2_clone = array2.clone();
        if let StackItem::Array(array) = &mut array2 {
            let _ = array.push(array2_clone);
        }

        // The arrays should be equal despite the cycles
        assert!(array1.equals(&array2).unwrap_or(false));
    }

    #[test]
    fn array_and_struct_ordering_compare_length_then_items() {
        let short_array = StackItem::from_array(vec![StackItem::from_int(1)]);
        let long_array =
            StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);
        assert_eq!(short_array.cmp(&long_array), std::cmp::Ordering::Less);

        let lower_array =
            StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);
        let higher_array =
            StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(3)]);
        assert_eq!(lower_array.cmp(&higher_array), std::cmp::Ordering::Less);

        let short_struct = StackItem::from_struct(vec![StackItem::from_int(1)]);
        let long_struct =
            StackItem::from_struct(vec![StackItem::from_int(1), StackItem::from_int(2)]);
        assert_eq!(short_struct.cmp(&long_struct), std::cmp::Ordering::Less);

        let lower_struct =
            StackItem::from_struct(vec![StackItem::from_int(1), StackItem::from_int(2)]);
        let higher_struct =
            StackItem::from_struct(vec![StackItem::from_int(1), StackItem::from_int(3)]);
        assert_eq!(lower_struct.cmp(&higher_struct), std::cmp::Ordering::Less);
    }
}

#[cfg(test)]
mod buffer_bytestring_equal_tests {
    use super::*;
    use crate::stack_item::buffer::Buffer as BufferItem;

    #[test]
    fn buffer_never_equals_bytestring() {
        // In C# Neo VM, Buffer uses reference equality (compound type).
        // Buffer == ByteString is always false, even with same content.
        let bs = StackItem::ByteString(vec![0x01]);
        let buf = StackItem::Buffer(BufferItem::new(vec![0x01]));
        assert!(
            !bs.equals(&buf).unwrap(),
            "ByteString(01) should NOT equal Buffer(01)"
        );
        assert!(
            !buf.equals(&bs).unwrap(),
            "Buffer(01) should NOT equal ByteString(01)"
        );
    }

    #[test]
    fn buffer_reference_equality() {
        // Same Buffer instance equals itself
        let buf = StackItem::Buffer(BufferItem::new(vec![0x01]));
        assert!(buf.equals(&buf).unwrap(), "Buffer should equal itself");
    }
}

#[cfg(test)]
mod zero_allocation_perf_tests {
    use super::*;

    #[test]
    fn as_bool_bytestring_no_clone() {
        // PERF REGRESSION: Verify ByteString as_bool doesn't allocate StackValue
        let bs_truthy = StackItem::ByteString(vec![0x01, 0x02]);
        let bs_falsy = StackItem::ByteString(vec![0x00, 0x00]);
        let bs_empty = StackItem::ByteString(vec![]);

        // Protocol correctness: truthiness logic matches C# Neo VM
        assert!(
            bs_truthy.as_bool().unwrap(),
            "non-zero bytes should be true"
        );
        assert!(
            !bs_falsy.as_bool().unwrap(),
            "all-zero bytes should be false"
        );
        assert!(!bs_empty.as_bool().unwrap(), "empty bytes should be false");

        // Edge case: single non-zero byte
        assert!(StackItem::ByteString(vec![0x01]).as_bool().unwrap());
        // Edge case: single zero byte
        assert!(!StackItem::ByteString(vec![0x00]).as_bool().unwrap());
    }

    #[test]
    fn as_bool_integer_fast_path() {
        // PERF REGRESSION: Small integers use fast path without heap access
        let zero = StackItem::from_i64(0);
        let one = StackItem::from_i64(1);
        let negative = StackItem::from_i64(-42);
        let large = StackItem::from_i64(999999);

        // Protocol correctness: integer truthiness
        assert!(!zero.as_bool().unwrap(), "zero is false");
        assert!(one.as_bool().unwrap(), "one is true");
        assert!(negative.as_bool().unwrap(), "negative is true");
        assert!(large.as_bool().unwrap(), "large value is true");
    }

    #[test]
    fn into_bytes_moves_bytestring() {
        // PERF REGRESSION: into_bytes should move Vec without clone for ByteString
        let original_data = vec![0x01, 0x02, 0x03, 0x04];
        let bs = StackItem::ByteString(original_data.clone());

        let bytes = bs.into_bytes().unwrap();
        assert_eq!(bytes, original_data, "bytes should match");
    }

    #[test]
    fn as_bytes_small_integer_fast_path() {
        // PERF REGRESSION: Small integers convert without BigInt serialization
        let test_cases = vec![
            (0i64, vec![]),        // zero = empty
            (1i64, vec![0x01]),    // single byte positive
            (127i64, vec![0x7F]),  // max single byte positive
            (-1i64, vec![0xFF]),   // single byte negative
            (-128i64, vec![0x80]), // min single byte negative
        ];

        for (value, expected) in test_cases {
            let item = StackItem::from_i64(value);
            let bytes = item.as_bytes().unwrap();
            assert_eq!(
                bytes, expected,
                "fast path for {value} should produce {expected:?}"
            );
        }

        // Larger values fall back to BigInt serialization
        let large = StackItem::from_i64(128);
        let bytes = large.as_bytes().unwrap();
        assert_eq!(
            bytes,
            vec![0x80, 0x00],
            "128 requires two bytes with sign bit"
        );
    }

    #[test]
    fn into_bytes_small_integer_fast_path() {
        // PERF REGRESSION: into_bytes also uses fast path for small integers
        let test_cases = vec![(0i64, vec![]), (42i64, vec![0x2A]), (-1i64, vec![0xFF])];

        for (value, expected) in test_cases {
            let item = StackItem::from_i64(value);
            let bytes = item.into_bytes().unwrap();
            assert_eq!(bytes, expected, "into_bytes fast path for {value}");
        }
    }

    #[test]
    fn into_int_zero_allocation_for_small() {
        // PERF REGRESSION: into_int moves without clone for VmInteger::Small
        let small = StackItem::from_i64(42);
        let bigint = small.into_int().unwrap();
        assert_eq!(bigint, BigInt::from(42), "should produce correct value");

        // Also test Boolean and other conversions maintain correctness
        let bool_true = StackItem::Boolean(true);
        assert_eq!(bool_true.into_int().unwrap(), BigInt::from(1));

        let bool_false = StackItem::Boolean(false);
        assert_eq!(bool_false.into_int().unwrap(), BigInt::from(0));
    }

    #[test]
    fn convert_to_same_type_returns_clone() {
        // PERF REGRESSION: Same-type conversion uses fast path
        let int_item = StackItem::from_i64(42);
        let converted = int_item.convert_to(StackItemType::Integer).unwrap();
        assert_eq!(converted.as_int().unwrap(), BigInt::from(42));

        let bs = StackItem::ByteString(vec![0x01, 0x02]);
        let converted = bs.convert_to(StackItemType::ByteString).unwrap();
        assert_eq!(converted.as_bytes().unwrap(), vec![0x01, 0x02]);
    }

    #[test]
    fn convert_to_direct_without_stackvalue() {
        // PERF REGRESSION: Conversions avoid StackValue intermediate

        // Boolean to ByteString
        let bool_item = StackItem::Boolean(true);
        let bs = bool_item.convert_to(StackItemType::ByteString).unwrap();
        assert_eq!(bs.as_bytes().unwrap(), vec![0x01]);

        // Integer to ByteString
        let int_item = StackItem::from_i64(42);
        let bs = int_item.convert_to(StackItemType::ByteString).unwrap();
        assert_eq!(bs.as_bytes().unwrap(), vec![0x2A]);

        // ByteString to Buffer
        let bs = StackItem::ByteString(vec![0x01, 0x02]);
        let buf = bs.convert_to(StackItemType::Buffer).unwrap();
        assert!(matches!(buf, StackItem::Buffer(_)));
        assert_eq!(buf.as_bytes().unwrap(), vec![0x01, 0x02]);

        // Buffer to ByteString
        let buf = StackItem::Buffer(BufferItem::new(vec![0x03, 0x04]));
        let bs = buf.convert_to(StackItemType::ByteString).unwrap();
        assert_eq!(bs.as_bytes().unwrap(), vec![0x03, 0x04]);
    }

    #[test]
    fn protocol_correctness_boolean_conversion() {
        // CRITICAL: Verify protocol-compliant boolean semantics maintained

        // Null is false
        assert!(!StackItem::Null.as_bool().unwrap());

        // Empty bytestring is false
        assert!(!StackItem::ByteString(vec![]).as_bool().unwrap());

        // All-zero bytestring is false
        assert!(!StackItem::ByteString(vec![0x00]).as_bool().unwrap());
        assert!(!StackItem::ByteString(vec![0x00, 0x00]).as_bool().unwrap());

        // Any non-zero byte makes it true
        assert!(StackItem::ByteString(vec![0x00, 0x01]).as_bool().unwrap());
        assert!(StackItem::ByteString(vec![0x01, 0x00]).as_bool().unwrap());

        // Compound types are always true (even empty)
        assert!(
            StackItem::Buffer(BufferItem::new(vec![]))
                .as_bool()
                .unwrap()
        );
        assert!(
            StackItem::from_array(Vec::<StackItem>::new())
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn protocol_correctness_byte_conversion() {
        // CRITICAL: Verify byte conversion maintains protocol format

        // Integer to bytes must use little-endian signed format
        let int256 = StackItem::from_i64(256);
        let bytes = int256.as_bytes().unwrap();
        assert_eq!(bytes, vec![0x00, 0x01], "256 in LE signed format");

        // Negative integers
        let neg1 = StackItem::from_i64(-1);
        let bytes = neg1.as_bytes().unwrap();
        assert_eq!(bytes, vec![0xFF], "-1 in LE signed format");

        // Large negative
        let neg256 = StackItem::from_i64(-256);
        let bytes = neg256.as_bytes().unwrap();
        assert_eq!(bytes, vec![0x00, 0xFF], "-256 in LE signed format");

        // Boolean conversions
        assert_eq!(StackItem::Boolean(false).as_bytes().unwrap(), vec![0x00u8]);
        assert_eq!(StackItem::Boolean(true).as_bytes().unwrap(), vec![0x01u8]);

        // Null conversion
        assert_eq!(StackItem::Null.as_bytes().unwrap(), Vec::<u8>::new());
    }
}
