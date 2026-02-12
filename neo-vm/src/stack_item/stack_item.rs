#![allow(clippy::mutable_key_type)]

//! Stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the stack item implementations used in the Neo VM.

use crate::collections::VmOrderedDictionary;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine_limits::ExecutionEngineLimits;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::stack_item::array::Array as ArrayItem;
use crate::stack_item::buffer::Buffer as BufferItem;
use crate::stack_item::map::Map as MapItem;
use crate::stack_item::pointer::Pointer as PointerItem;
use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::struct_item::Struct as StructItem;
use num_bigint::BigInt;
use num_traits::Zero;
use std::fmt;
use std::sync::Arc;

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

/// A trait for interop interfaces that can be wrapped by a `stack_item`.
pub trait InteropInterface: fmt::Debug + Send + Sync {
    /// Gets the type of the interop interface.
    fn interface_type(&self) -> &str;

    /// Allows downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Represents a value in the Neo VM.
#[derive(Debug, Clone)]
pub enum StackItem {
    /// Represents a null value.
    Null,

    /// Represents a boolean value.
    Boolean(bool),

    /// Represents an integer value.
    Integer(BigInt),

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
        Self::Integer(value.into())
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
            Self::Null => Ok(false),
            Self::Boolean(b) => Ok(*b),
            Self::Integer(i) => Ok(!i.is_zero()),
            Self::ByteString(b) => {
                if b.len() > crate::stack_item::integer::Integer::MAX_SIZE {
                    return Err(VmError::invalid_type_simple(
                        "Cannot convert ByteString to Boolean",
                    ));
                }
                Ok(b.iter().any(|&byte| byte != 0))
            }
            Self::Buffer(_b) => Ok(true),
            Self::Array(_a) => Ok(true),
            Self::Struct(_s) => Ok(true),
            Self::Map(_m) => Ok(true),
            Self::Pointer(_pointer) => Ok(true),
            Self::InteropInterface(_i) => Ok(true),
        }
    }

    /// Converts the stack item to an integer.
    #[inline]
    pub fn as_int(&self) -> VmResult<BigInt> {
        match self {
            Self::Null => Err(VmError::invalid_type_simple(
                "Cannot convert Null to Integer",
            )),
            Self::Boolean(b) => Ok(BigInt::from(i32::from(*b))),
            Self::Integer(i) => Ok(i.clone()),
            Self::ByteString(b) => {
                if b.len() > crate::stack_item::integer::Integer::MAX_SIZE {
                    return Err(VmError::invalid_type_simple(
                        "Cannot convert ByteString to Integer",
                    ));
                }
                if b.is_empty() {
                    return Ok(BigInt::from(0));
                }

                let bytes = b.clone();
                let is_negative = (bytes[bytes.len() - 1] & 0x80) != 0;
                if is_negative {
                    let mut bytes_copy = bytes;
                    let len = bytes_copy.len();
                    bytes_copy[len - 1] &= 0x7F;
                    let positive_value = BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes_copy);
                    let sign_bit_value = BigInt::from(1) << (len * 8 - 1);
                    Ok(-(sign_bit_value - positive_value))
                } else {
                    Ok(BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes))
                }
            }
            Self::Buffer(b) => {
                if b.len() > crate::stack_item::integer::Integer::MAX_SIZE {
                    return Err(VmError::invalid_type_simple(
                        "Cannot convert Buffer to Integer",
                    ));
                }
                if b.is_empty() {
                    return Ok(BigInt::from(0));
                }

                let bytes = b.data();
                let is_negative = (bytes[bytes.len() - 1] & 0x80) != 0;
                if is_negative {
                    let mut bytes_copy = bytes;
                    let len = bytes_copy.len();
                    bytes_copy[len - 1] &= 0x7F;
                    let positive_value = BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes_copy);
                    let sign_bit_value = BigInt::from(1) << (len * 8 - 1);
                    Ok(-(sign_bit_value - positive_value))
                } else {
                    Ok(BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes))
                }
            }
            _ => Err(VmError::invalid_type_simple("Cannot convert to Integer")),
        }
    }

    /// Returns the boolean value represented by the stack item.
    #[inline]
    pub fn get_boolean(&self) -> VmResult<bool> {
        self.as_bool()
    }

    /// Returns the integer value represented by the stack item.
    #[inline]
    pub fn get_integer(&self) -> VmResult<BigInt> {
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
            Self::Null => Ok(Vec::new()),
            Self::Boolean(b) => Ok(vec![u8::from(*b)]),
            Self::Integer(i) => Ok(normalize_bigint_bytes(i)),
            Self::ByteString(b) => Ok(b.clone()),
            Self::Buffer(b) => Ok(b.data()),
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
    pub fn get_hash_code(&self) -> i32 {
        match self {
            Self::Null => 0,
            Self::Boolean(b) => i32::from(*b),
            Self::Integer(i) => hash_bytes(&i.to_signed_bytes_le()),
            Self::ByteString(b) => hash_bytes(b),
            Self::Buffer(b) => hash_bytes(&b.data()),
            Self::Array(array) => {
                let mut hash = combine_hash(17, array.len() as i32);
                for item in array {
                    hash = combine_hash(hash, item.get_hash_code());
                }
                hash
            }
            Self::Struct(structure) => {
                let mut hash = combine_hash(17, structure.len() as i32);
                for item in structure.items() {
                    hash = combine_hash(hash, item.get_hash_code());
                }
                hash
            }
            Self::Map(map) => {
                let mut hash = combine_hash(17, map.len() as i32);
                for (key, value) in map.items().iter() {
                    hash = combine_hash(hash, key.get_hash_code());
                    hash = combine_hash(hash, value.get_hash_code());
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

        match item_type {
            StackItemType::Boolean => Ok(Self::Boolean(self.as_bool()?)),
            StackItemType::Integer => Ok(Self::Integer(self.as_int()?)),
            StackItemType::ByteString => Ok(Self::ByteString(self.as_bytes()?)),
            StackItemType::Buffer => Ok(Self::Buffer(BufferItem::new(self.as_bytes()?))),
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
    pub fn equals_with_limits(
        &self,
        other: &Self,
        _limits: &ExecutionEngineLimits,
    ) -> VmResult<bool> {
        self.equals(other)
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

        let result = match (self, other) {
            (Self::Null, Self::Null) => Ok(true),
            (Self::Boolean(a), Self::Boolean(b)) => Ok(a == b),
            (Self::Integer(a), Self::Integer(b)) => Ok(a == b),
            (Self::ByteString(a), Self::ByteString(b)) => Ok(a == b),
            (Self::Buffer(a), Self::Buffer(b)) => Ok(a == b),
            (Self::ByteString(a), Self::Buffer(b)) => Ok(a.as_slice() == b.data().as_slice()),
            (Self::Buffer(a), Self::ByteString(b)) => Ok(a.data().as_slice() == b.as_slice()),
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

fn normalize_bigint_bytes(value: &BigInt) -> Vec<u8> {
    if value.is_zero() {
        return vec![];
    }

    let mut bytes = value.to_signed_bytes_le();
    let negative = value.sign() == num_bigint::Sign::Minus;

    if let Some(last) = bytes.last() {
        let sign_bit_set = last & 0x80 != 0;
        if !negative && sign_bit_set {
            bytes.push(0);
        } else if negative && !sign_bit_set {
            bytes.push(0xFF);
        }
    }

    bytes
}

// Implement PartialEq to allow stack items to be compared and used as keys in collections
impl PartialEq for StackItem {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other).unwrap_or(false)
    }
}

impl Eq for StackItem {}

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
        let type_order = (self.stack_item_type() as u8).cmp(&(other.stack_item_type() as u8));
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
            (Self::ByteString(a), Self::Buffer(b)) => a.as_slice().cmp(b.data().as_slice()),
            (Self::Buffer(a), Self::ByteString(b)) => a.data().as_slice().cmp(b.as_slice()),
            (Self::Pointer(a), Self::Pointer(b)) => a.cmp(b),
            (Self::Array(a), Self::Array(b)) => {
                let len_cmp = a.len().cmp(&b.len());
                if len_cmp != std::cmp::Ordering::Equal {
                    return len_cmp;
                }
                for (item_a, item_b) in a.iter().zip(b.iter()) {
                    let item_cmp = item_a.cmp(&item_b);
                    if item_cmp != std::cmp::Ordering::Equal {
                        return item_cmp;
                    }
                }
                std::cmp::Ordering::Equal
            }
            (Self::Struct(a), Self::Struct(b)) => {
                let len_cmp = a.len().cmp(&b.len());
                if len_cmp != std::cmp::Ordering::Equal {
                    return len_cmp;
                }
                for (item_a, item_b) in a.iter().zip(b.iter()) {
                    let item_cmp = item_a.cmp(&item_b);
                    if item_cmp != std::cmp::Ordering::Equal {
                        return item_cmp;
                    }
                }
                std::cmp::Ordering::Equal
            }
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
}
