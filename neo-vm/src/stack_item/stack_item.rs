#![allow(clippy::mutable_key_type)]

//! Stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the stack item implementations used in the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::stack_item::array::Array as ArrayItem;
use crate::stack_item::buffer::Buffer as BufferItem;
use crate::stack_item::map::Map as MapItem;
use crate::stack_item::pointer::Pointer as PointerItem;
use crate::stack_item::struct_item::Struct as StructItem;
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::StackItemType;
use neo_vm_rs::{StackValue, VmOrderedDictionary};
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

pub(crate) fn decode_integer_bytes(data: &[u8]) -> VmResult<BigInt> {
    if data.len() > VM_INTEGER_MAX_SIZE {
        return Err(VmError::invalid_type_simple("integer size exceeds maximum"));
    }
    Ok(BigInt::from_signed_bytes_le(data))
}

#[inline]
fn stack_value_truthy(value: StackValue) -> bool {
    neo_vm_rs::semantics::comparison::boolean_value(&value)
}

fn convert_stack_value_with_neo_vm_rs(
    value: StackValue,
    target_type: StackItemType,
) -> VmResult<StackItem> {
    let converted = neo_vm_rs::semantics::conversion::convert_value(value, target_type.to_byte())
        .map_err(VmError::invalid_type_simple)?;
    StackItem::try_from(converted)
}

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

mod equality;

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
    #[inline]
    pub fn as_bool(&self) -> VmResult<bool> {
        match self {
            Self::Null => Ok(stack_value_truthy(StackValue::Null)),
            Self::Boolean(b) => Ok(stack_value_truthy(StackValue::Boolean(*b))),
            Self::Integer(i) => Ok(stack_value_truthy(i.vm_integer_stack_value())),
            Self::ByteString(b) => {
                if b.len() > VM_INTEGER_MAX_SIZE {
                    return Err(VmError::invalid_type_simple(
                        "Cannot convert ByteString to Boolean",
                    ));
                }
                // NeoVM truthiness: true iff any byte is non-zero (matches
                // neo_vm_rs boolean_value / C# Unsafe.NotZero). Avoids cloning the
                // Vec<u8> just to wrap it in StackValue::ByteString.
                Ok(b.iter().any(|byte| *byte != 0))
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
    #[inline]
    pub fn as_int(&self) -> VmResult<BigInt> {
        match self {
            Self::Null => Err(VmError::invalid_type_simple(
                "Cannot convert Null to Integer",
            )),
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
                buf.with_data(Self::bytes_to_bigint)
            }
            _ => Err(VmError::invalid_type_simple("Cannot convert to Integer")),
        }
    }

    /// Consuming version of `as_int` — moves the BigInt out of an Integer
    /// variant instead of cloning. Use when the StackItem is already owned
    /// (e.g., after `pop()`).
    #[inline]
    pub fn into_int(self) -> VmResult<BigInt> {
        match self {
            Self::Null => Err(VmError::invalid_type_simple(
                "Cannot convert Null to Integer",
            )),
            Self::Boolean(b) => Ok(BigInt::from(i32::from(b))),
            Self::Integer(i) => Ok(i.into_bigint()), // MOVE — no clone for Small!
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
                buf.with_data(Self::bytes_to_bigint)
            }
            _ => Err(VmError::invalid_type_simple("Cannot convert to Integer")),
        }
    }

    /// Shared helper: convert byte slice to BigInt with NeoVM integer rules.
    fn bytes_to_bigint(data: &[u8]) -> VmResult<BigInt> {
        decode_integer_bytes(data)
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
        Self::bytes_to_bigint(b)
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
    #[inline]
    pub fn as_bytes(&self) -> VmResult<Vec<u8>> {
        match self {
            Self::Null
            | Self::Boolean(_)
            | Self::Integer(_)
            | Self::ByteString(_)
            | Self::Buffer(_) => {
                stack_value_byte_string_bytes(neo_vm_rs::StackValue::try_from(self.clone())?)
            }
            _ => Err(VmError::invalid_type_simple("Cannot convert to ByteArray")),
        }
    }

    /// Consuming version of `as_bytes` — moves the Vec out of ByteString
    /// instead of cloning. Use when the StackItem is already owned (e.g., after `pop()`).
    #[inline]
    pub fn into_bytes(self) -> VmResult<Vec<u8>> {
        match self {
            item @ (Self::Null
            | Self::Boolean(_)
            | Self::Integer(_)
            | Self::ByteString(_)
            | Self::Buffer(_)) => {
                stack_value_byte_string_bytes(neo_vm_rs::StackValue::try_from(item)?)
            }
            _ => Err(VmError::invalid_type_simple("Cannot convert to ByteArray")),
        }
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
        if let Some(self_id) = compound_identity(self) {
            if let Some(cloned) = refs.get(&self_id) {
                return cloned.clone();
            }
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
    pub fn convert_to(&self, item_type: StackItemType) -> VmResult<Self> {
        if self.stack_item_type() == item_type {
            return Ok(self.clone());
        }

        match (self, item_type) {
            (
                Self::Null | Self::Integer(_) | Self::ByteString(_),
                target_type @ StackItemType::Boolean,
            ) => {
                if let Self::ByteString(bytes) = self {
                    if bytes.len() > VM_INTEGER_MAX_SIZE {
                        return Err(VmError::invalid_type_simple(
                            "Cannot convert ByteString to Boolean",
                        ));
                    }
                }
                return convert_stack_value_with_neo_vm_rs(
                    StackValue::try_from(self.clone())?,
                    target_type,
                );
            }
            (
                Self::Boolean(_) | Self::Integer(_) | Self::ByteString(_) | Self::Buffer(_),
                target_type @ (StackItemType::ByteString | StackItemType::Buffer),
            ) => {
                return convert_stack_value_with_neo_vm_rs(
                    StackValue::try_from(self.clone())?,
                    target_type,
                );
            }
            (Self::Null, StackItemType::ByteString) => {
                return Ok(Self::ByteString(self.as_bytes()?));
            }
            (Self::Null, StackItemType::Buffer) => {
                return Ok(Self::Buffer(BufferItem::new(self.as_bytes()?)));
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

fn stack_value_byte_string_bytes(value: neo_vm_rs::StackValue) -> VmResult<Vec<u8>> {
    value
        .to_byte_string_bytes()
        .ok_or_else(|| VmError::invalid_type_simple("Cannot convert to ByteArray"))
}

// Implement PartialEq to allow stack items to be compared and used as keys in collections
impl PartialEq for StackItem {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other).unwrap_or(false)
    }
}

impl Eq for StackItem {}

impl TryFrom<neo_vm_rs::StackValue> for StackItem {
    type Error = VmError;

    fn try_from(value: neo_vm_rs::StackValue) -> VmResult<Self> {
        match value {
            neo_vm_rs::StackValue::Integer(value) => Ok(Self::from_i64(value)),
            neo_vm_rs::StackValue::BigInteger(bytes) => {
                let value = decode_integer_bytes(&bytes)?;
                Ok(Self::from_int(value))
            }
            neo_vm_rs::StackValue::ByteString(bytes) => Ok(Self::from_byte_string(bytes)),
            neo_vm_rs::StackValue::Buffer(bytes) => Ok(Self::from_buffer(bytes)),
            neo_vm_rs::StackValue::Boolean(value) => Ok(Self::from_bool(value)),
            neo_vm_rs::StackValue::Array(items) => {
                let items = items
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::from_array(items))
            }
            neo_vm_rs::StackValue::Struct(items) => {
                let items = items
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::from_struct(items))
            }
            neo_vm_rs::StackValue::Map(entries) => {
                let mut map = VmOrderedDictionary::with_capacity(entries.len());
                for (key, value) in entries {
                    map.insert(Self::try_from(key)?, Self::try_from(value)?);
                }
                Ok(Self::from_map(map))
            }
            neo_vm_rs::StackValue::Null => Ok(Self::Null),
            neo_vm_rs::StackValue::Pointer(_)
            | neo_vm_rs::StackValue::Interop(_)
            | neo_vm_rs::StackValue::Iterator(_) => Err(VmError::invalid_operation_msg(format!(
                "Cannot convert {:?} into neo-vm StackItem without host runtime identity",
                value
            ))),
        }
    }
}

impl TryFrom<StackItem> for neo_vm_rs::StackValue {
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
                // Different variants: order by variant rank.
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
#[path = "../tests/stack_item/stack_item.rs"]
mod tests;
