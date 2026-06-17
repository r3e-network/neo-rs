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

/// Implements the C# base `StackItem.Equals(other)` virtual dispatch used by the
/// `EQUAL`/`NOTEQUAL` opcodes (`JumpTable.Bitwisee.cs:89` → `x1.Equals(x2, limits)`).
///
/// This is the non-faulting comparison path. It mirrors each concrete type's
/// `Equals(StackItem)` override in `neo_csharp_vm/src/Neo.VM/Types`:
/// - `Integer`/`Boolean`: value equality within the same concrete type, else false
///   (TYPE-STRICT — `Integer(1) != ByteString([1])`, verified against mainnet C# v3.9.1).
/// - `ByteString`: byte equality within the same type (no budget here; the budgeted
///   variant is handled directly by [`StackItem::equals_with_limits`]).
/// - `Pointer`: position + originating-script equality (`Pointer.cs:46-51`).
/// - `Null`: `other is Null` (`Null.cs:38-42`).
/// - `Array`/`Map`/`Buffer`/`InteropInterface`: REFERENCE equality, because these
///   types do NOT override `Equals` and fall back to the base
///   `ReferenceEquals(this, other)` (`StackItem.cs:117-120`). NOTE: this differs from
///   the structural equality used by [`StackItem::equals`] (Rust `PartialEq`, used for
///   map keys / reference counting), which must stay structural.
fn equals_plain(a: &StackItem, b: &StackItem) -> bool {
    match (a, b) {
        (StackItem::Null, StackItem::Null) => true,
        (StackItem::Boolean(x), StackItem::Boolean(y)) => x == y,
        (StackItem::Integer(x), StackItem::Integer(y)) => x.to_bigint() == y.to_bigint(),
        (StackItem::ByteString(x), StackItem::ByteString(y)) => x == y,
        (StackItem::Pointer(x), StackItem::Pointer(y)) => x == y,
        (StackItem::InteropInterface(x), StackItem::InteropInterface(y)) => Arc::ptr_eq(x, y),
        (StackItem::Buffer(x), StackItem::Buffer(y)) => x.id() == y.id(),
        // Array/Map/Struct fall back to the base `ReferenceEquals` (identity).
        (StackItem::Array(_), _) | (StackItem::Struct(_), _) | (StackItem::Map(_), _) => {
            match (compound_identity(a), compound_identity(b)) {
                (Some(ia), Some(ib)) => ia == ib,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Implements C# `ByteString.Equals(other, ref limits)` (`ByteString.cs:60-78`).
///
/// Faults (returns `Err`) when `self`'s size exceeds the remaining budget or the
/// budget is already exhausted, and decrements the budget by the compared size.
fn byte_string_size_eq_with_budget(
    a: &[u8],
    other: &StackItem,
    limits: &mut u32,
) -> VmResult<bool> {
    let a_size = a.len() as u64;
    if a_size > u64::from(*limits) || *limits == 0 {
        return Err(VmError::invalid_operation_msg(format!(
            "The operand exceeds the maximum comparable size, {a_size}/{limits}."
        )));
    }
    // comparedSize starts at 1 (C# `uint comparedSize = 1;`).
    let mut compared_size: u64 = 1;
    let result = match other {
        StackItem::ByteString(b) => {
            compared_size = compared_size.max(a_size).max(b.len() as u64);
            if (b.len() as u64) > u64::from(*limits) {
                // Decrement still runs in C#'s `finally` before the throw propagates,
                // but the throw fails the engine regardless, so surface the fault here.
                return Err(VmError::invalid_operation_msg(format!(
                    "The operand exceeds the maximum comparable size, {}/{limits}.",
                    b.len()
                )));
            }
            Ok(a == b.as_slice())
        }
        // C# `other is not ByteString b` → return false (comparedSize stays 1).
        _ => Ok(false),
    };
    // C# `finally { limits -= comparedSize; }` — compared_size is bounded by *limits
    // on every non-faulting path, so the subtraction cannot underflow.
    *limits = limits.saturating_sub(compared_size as u32);
    result
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
        neo_vm_rs::decode_integer_bytes(data).map_err(VmError::invalid_type_simple)
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

    /// Checks if two stack items are equal.
    pub fn equals(&self, other: &Self) -> VmResult<bool> {
        self.equals_with_refs(other, &mut std::collections::HashSet::new())
    }

    /// Checks if two stack items are equal under the `EQUAL`/`NOTEQUAL` opcode rules.
    ///
    /// Faithful port of C# `StackItem.Equals(other, ExecutionEngineLimits)` dispatch,
    /// invoked from `JumpTable.Bitwisee.cs:89` (`x1.Equals(x2, engine.Limits)`). The
    /// comparison is dispatched on `self`'s concrete type:
    /// - `ByteString` → `ByteString.cs:54-78`: size/budget-checked byte equality
    ///   (FAULTS when either operand exceeds `MaxComparableSize`).
    /// - `Struct` → `Struct.cs:91-132`: iterative two-stack structural walk bounded by
    ///   `MaxStackSize` (item count) and `MaxComparableSize` (comparable budget); FAULTS
    ///   on overflow of either budget.
    /// - everything else → base `StackItem.Equals(other)` ([`equals_plain`]): value
    ///   equality for `Integer`/`Boolean`/`ByteString`/`Pointer`, `other is Null` for
    ///   `Null`, and REFERENCE equality for `Array`/`Map`/`Buffer`/`InteropInterface`.
    ///
    /// This intentionally differs from [`StackItem::equals`] (the structural `PartialEq`
    /// used for map keys and reference counting): under `EQUAL`, `Array`/`Map` use
    /// REFERENCE semantics, matching the C# reference VM.
    pub fn equals_with_limits(
        &self,
        other: &Self,
        limits: &ExecutionEngineLimits,
    ) -> VmResult<bool> {
        match self {
            Self::ByteString(bytes) => {
                let mut budget = limits.max_comparable_size;
                byte_string_size_eq_with_budget(bytes, other, &mut budget)
            }
            Self::Struct(_) => self.struct_equals_with_limits(other, limits),
            _ => Ok(equals_plain(self, other)),
        }
    }

    /// Port of C# `Struct.Equals(other, ExecutionEngineLimits)` (`Struct.cs:91-132`).
    ///
    /// Iterative two-stack walk that bounds the number of compared items by
    /// `MaxStackSize` and the cumulative comparable size by `MaxComparableSize`,
    /// faulting (`Err`) when either budget is exceeded.
    fn struct_equals_with_limits(
        &self,
        other: &Self,
        limits: &ExecutionEngineLimits,
    ) -> VmResult<bool> {
        // `other is not Struct s => return false`
        let other_struct = match other {
            Self::Struct(s) => s,
            _ => return Ok(false),
        };
        let self_struct = match self {
            Self::Struct(s) => s,
            // Unreachable: only called with `self` being a Struct.
            _ => return Ok(false),
        };

        let mut stack1: Vec<StackItem> = vec![Self::Struct(self_struct.clone())];
        let mut stack2: Vec<StackItem> = vec![Self::Struct(other_struct.clone())];
        let mut count = limits.max_stack_size;
        let mut max_comparable_size = limits.max_comparable_size;

        while let Some(a) = stack1.pop() {
            // C# `if (count-- == 0) throw` — fault once the item budget is exhausted.
            if count == 0 {
                return Err(VmError::invalid_operation_msg(
                    "Too many struct items to compare in struct comparison.",
                ));
            }
            count -= 1;

            let b = stack2.pop().ok_or_else(|| {
                VmError::invalid_operation_msg("Struct comparison stack underflow")
            })?;

            if let Self::ByteString(bytes) = &a {
                if !byte_string_size_eq_with_budget(bytes, &b, &mut max_comparable_size)? {
                    return Ok(false);
                }
            } else {
                // C# `if (maxComparableSize == 0) throw; maxComparableSize -= 1;`
                if max_comparable_size == 0 {
                    return Err(VmError::invalid_operation_msg(
                        "The operand exceeds the maximum comparable size in struct comparison.",
                    ));
                }
                max_comparable_size -= 1;

                if let Self::Struct(sa) = &a {
                    // `if (ReferenceEquals(a, b)) continue;`
                    if let Self::Struct(sb) = &b {
                        if sa.id() == sb.id() {
                            continue;
                        }
                        // `if (sa.Count != sb.Count) return false;`
                        if sa.len() != sb.len() {
                            return Ok(false);
                        }
                        for item in sa.iter() {
                            stack1.push(item);
                        }
                        for item in sb.iter() {
                            stack2.push(item);
                        }
                    } else {
                        // `if (b is not Struct sb) return false;`
                        return Ok(false);
                    }
                } else {
                    // C# base virtual `a.Equals(b)` (reference for compounds, value otherwise).
                    if !equals_plain(&a, &b) {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
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
                let value = neo_vm_rs::decode_integer_bytes(&bytes)
                    .map_err(VmError::invalid_type_simple)?;
                Ok(Self::from_int(value))
            }
            neo_vm_rs::StackValue::ByteString(bytes) => Ok(Self::from_byte_string(bytes)),
            neo_vm_rs::StackValue::Buffer(_, bytes) => Ok(Self::from_buffer(bytes)),
            neo_vm_rs::StackValue::Boolean(value) => Ok(Self::from_bool(value)),
            neo_vm_rs::StackValue::Array(_, items) => {
                let items = items
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::from_array(items))
            }
            neo_vm_rs::StackValue::Struct(_, items) => {
                let items = items
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::from_struct(items))
            }
            neo_vm_rs::StackValue::Map(_, entries) => {
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
            StackItem::Buffer(buffer) => Ok(Self::Buffer(0, buffer.data())),
            StackItem::Array(array) => {
                let items = array
                    .items()
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::Array(0, items))
            }
            StackItem::Struct(structure) => {
                let items = structure
                    .items()
                    .into_iter()
                    .map(Self::try_from)
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::Struct(0, items))
            }
            StackItem::Map(map) => {
                let entries = map
                    .iter()
                    .map(|(key, value)| Ok((Self::try_from(key)?, Self::try_from(value)?)))
                    .collect::<VmResult<Vec<_>>>()?;
                Ok(Self::Map(0, entries))
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
    fn stack_value_big_integer_conversion_enforces_vm_integer_max_size() {
        let negative = StackItem::try_from(neo_vm_rs::StackValue::BigInteger(vec![0xff]))
            .expect("valid BigInteger bytes should convert");
        assert_eq!(negative.as_int().unwrap(), BigInt::from(-1));

        let too_large = neo_vm_rs::StackValue::BigInteger(vec![0x01; VM_INTEGER_MAX_SIZE + 1]);
        assert!(StackItem::try_from(too_large).is_err());
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

    #[test]
    fn equal_opcode_faults_on_oversized_byte_strings() {
        // C# ByteString.Equals(other, ref limits) throws when Size > MaxComparableSize
        // (ByteString.cs:62). MaxComparableSize default is 65536.
        let limits = ExecutionEngineLimits::default();
        let big = limits.max_comparable_size as usize + 1;
        let a = StackItem::from_byte_string(vec![0u8; big]);
        let b = StackItem::from_byte_string(vec![0u8; big]);

        assert!(
            a.equals_with_limits(&b, &limits).is_err(),
            "EQUAL on two >MaxComparableSize ByteStrings must fault"
        );
        // The same items still compare structurally via the un-limited PartialEq path.
        assert!(a.equals(&b).unwrap());
    }

    #[test]
    fn equal_opcode_faults_on_oversized_struct_item_count() {
        // C# Struct.Equals throws "Too many struct items" once more than MaxStackSize
        // items are popped during the walk (Struct.cs:102-103).
        let limits = ExecutionEngineLimits::default();
        let count = limits.max_stack_size as usize + 64;

        let a_inner: Vec<StackItem> = (0..count).map(|_| StackItem::from_int(1)).collect();
        let b_inner: Vec<StackItem> = (0..count).map(|_| StackItem::from_int(1)).collect();
        let a = StackItem::from_struct(a_inner);
        let b = StackItem::from_struct(b_inner);

        assert!(
            a.equals_with_limits(&b, &limits).is_err(),
            "EQUAL on a Struct with >MaxStackSize items must fault"
        );
    }

    #[test]
    fn equal_opcode_uses_reference_semantics_for_arrays() {
        // C# Array does NOT override Equals, so under EQUAL it falls back to
        // ReferenceEquals (StackItem.cs:117-120): structurally-identical but distinct
        // arrays compare NOT equal.
        let limits = ExecutionEngineLimits::default();
        let a = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);
        let b = StackItem::from_array(vec![StackItem::from_int(1), StackItem::from_int(2)]);

        assert!(
            !a.equals_with_limits(&b, &limits).unwrap(),
            "distinct equal-valued arrays must be NOT equal under EQUAL (reference semantics)"
        );
        // Same reference compares equal.
        assert!(a.equals_with_limits(&a.clone(), &limits).unwrap());
        // The structural PartialEq path is unchanged and still reports equal.
        assert!(a.equals(&b).unwrap());
    }

    #[test]
    fn equal_opcode_uses_reference_semantics_for_maps() {
        // C# Map likewise falls back to ReferenceEquals under EQUAL.
        let limits = ExecutionEngineLimits::default();
        let mut d1 = VmOrderedDictionary::new();
        d1.insert(StackItem::from_int(1), StackItem::from_int(2));
        let mut d2 = VmOrderedDictionary::new();
        d2.insert(StackItem::from_int(1), StackItem::from_int(2));
        let a = StackItem::from_map(d1);
        let b = StackItem::from_map(d2);

        assert!(
            !a.equals_with_limits(&b, &limits).unwrap(),
            "distinct equal-valued maps must be NOT equal under EQUAL (reference semantics)"
        );
        assert!(a.equals_with_limits(&a.clone(), &limits).unwrap());
    }

    #[test]
    fn equal_opcode_preserves_primitive_type_strictness() {
        // Verified against mainnet C# v3.9.1: Integer(1) != ByteString([1]) under EQUAL.
        let limits = ExecutionEngineLimits::default();
        let int_one = StackItem::from_int(1);
        let bytes_one = StackItem::from_byte_string(vec![1u8]);

        assert!(!int_one.equals_with_limits(&bytes_one, &limits).unwrap());
        // Dispatch is on self's type: ByteString.Equals(Integer) is also false.
        assert!(!bytes_one.equals_with_limits(&int_one, &limits).unwrap());
        // Same-type value equality still holds.
        assert!(
            int_one
                .equals_with_limits(&StackItem::from_int(1), &limits)
                .unwrap()
        );
        assert!(
            bytes_one
                .equals_with_limits(&StackItem::from_byte_string(vec![1u8]), &limits)
                .unwrap()
        );
    }

    #[test]
    fn equal_opcode_struct_byte_strings_share_comparable_budget() {
        // Inside Struct.Equals the byte-string budget is threaded across all nested
        // ByteString comparisons (Struct.cs:106-108). A struct whose nested byte
        // strings sum to more than MaxComparableSize must fault.
        let limits = ExecutionEngineLimits::default();
        let chunk = (limits.max_comparable_size / 2) as usize + 1;
        let a = StackItem::from_struct(vec![
            StackItem::from_byte_string(vec![0u8; chunk]),
            StackItem::from_byte_string(vec![0u8; chunk]),
        ]);
        let b = StackItem::from_struct(vec![
            StackItem::from_byte_string(vec![0u8; chunk]),
            StackItem::from_byte_string(vec![0u8; chunk]),
        ]);

        assert!(
            a.equals_with_limits(&b, &limits).is_err(),
            "nested byte strings exceeding the shared comparable budget must fault"
        );
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
