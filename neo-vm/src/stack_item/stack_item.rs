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
struct LimitBudget {
    /// Remaining `MaxStackSize` units; one is spent per compared struct sub item.
    remaining_stack: u64,
    /// Remaining `MaxComparableSize` units.
    remaining_comparable: i64,
    /// Struct pairs (by id) that are currently being compared.
    visited: std::collections::HashSet<(usize, usize)>,
}

/// Renders the item tree on an explicit work stack.
///
/// A derived formatter adds one native frame per nesting level, so an item
/// nested as deep as `MaxStackSize` (a legal depth) overflowed a 1 MiB thread
/// stack. The emitted text — including the wrapper shapes of the compound
/// items (`Type { inner: Mutex { data: TypeInner { .. } } }`) and their ids —
/// is the same as the previous derived output, so log and error-message text
/// does not change.
impl fmt::Debug for StackItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        /// One print job; the tree is walked by popping these.
        enum Task {
            /// Print a stack item at the given indentation column.
            Item(StackItem, usize),
            /// Already-rendered text.
            Text(String),
            /// A numeric field value.
            Num(usize),
            /// A boolean field value.
            Bool(bool),
            /// Newline followed by `n` spaces.
            Indent(usize),
        }

        fn text(value: &str) -> Task {
            Task::Text(value.to_string())
        }

        /// `value` with every line but the first shifted right by `indent`.
        ///
        /// The leaves are formatted by their own implementations at base
        /// indentation and then moved to their position in the tree.
        fn shift_lines(value: &str, indent: usize) -> String {
            if indent == 0 {
                return value.to_string();
            }
            let pad = " ".repeat(indent);
            let mut out = String::with_capacity(value.len() + pad.len() * 8);
            for (index, line) in value.split('\n').enumerate() {
                if index > 0 {
                    out.push('\n');
                    out.push_str(&pad);
                }
                out.push_str(line);
            }
            out
        }

        /// `Prefix(payload)` laid out the way the derived implementation lays
        /// it out at `indent`.
        fn leaf(prefix: &str, payload: &dyn fmt::Debug, pretty: bool, indent: usize) -> String {
            if !pretty {
                return format!("{prefix}({payload:?})");
            }
            let body = shift_lines(&format!("{payload:#?}"), indent + 4);
            format!(
                "{prefix}(\n{}{body},\n{})",
                " ".repeat(indent + 4),
                " ".repeat(indent)
            )
        }

        /// The elements of a list, as `Vec`'s implementation prints them.
        fn push_list(out: &mut Vec<Task>, items: Vec<StackItem>, indent: usize, pretty: bool) {
            if items.is_empty() {
                out.push(text("[]"));
                return;
            }
            out.push(text("["));
            if pretty {
                for item in items {
                    out.push(Task::Indent(indent + 4));
                    out.push(Task::Item(item, indent + 4));
                    out.push(text(","));
                }
                out.push(Task::Indent(indent));
            } else {
                for (index, item) in items.into_iter().enumerate() {
                    if index > 0 {
                        out.push(text(", "));
                    }
                    out.push(Task::Item(item, 0));
                }
            }
            out.push(text("]"));
        }

        fn push_num_field(out: &mut Vec<Task>, name: &str, value: usize, indent: usize) {
            out.push(Task::Indent(indent));
            out.push(Task::Text(format!("{name}: ")));
            out.push(Task::Num(value));
            out.push(text(","));
        }

        /// One `Array`/`Struct` node, in print order.
        fn list_node(
            kind: &str,
            items: Vec<StackItem>,
            id: usize,
            stack_references: usize,
            is_read_only: bool,
            indent: usize,
            pretty: bool,
        ) -> Vec<Task> {
            let mut out = Vec::new();
            if pretty {
                out.push(Task::Text(format!("{kind}(")));
                out.push(Task::Indent(indent + 4));
                out.push(Task::Text(format!("{kind} {{")));
                out.push(Task::Indent(indent + 8));
                out.push(text("inner: Mutex {"));
                out.push(Task::Indent(indent + 12));
                out.push(text("data: CompoundInner {"));
                out.push(Task::Indent(indent + 16));
                out.push(text("items: "));
                push_list(&mut out, items, indent + 16, pretty);
                out.push(text(","));
                push_num_field(&mut out, "stack_references", stack_references, indent + 16);
                push_num_field(&mut out, "id", id, indent + 16);
                out.push(Task::Indent(indent + 16));
                out.push(text("is_read_only: "));
                out.push(Task::Bool(is_read_only));
                out.push(text(","));
                out.push(Task::Indent(indent + 12));
                out.push(text("},"));
                out.push(Task::Indent(indent + 8));
                out.push(text("},"));
                out.push(Task::Indent(indent + 4));
                out.push(text("},"));
                out.push(Task::Indent(indent));
                out.push(text(")"));
                return out;
            }

            out.push(Task::Text(format!(
                "{kind}({kind} {{ inner: Mutex {{ data: CompoundInner {{ items: "
            )));
            push_list(&mut out, items, 0, pretty);
            out.push(text(", stack_references: "));
            out.push(Task::Num(stack_references));
            out.push(text(", id: "));
            out.push(Task::Num(id));
            out.push(text(", is_read_only: "));
            out.push(Task::Bool(is_read_only));
            out.push(text(" } } })"));
            out
        }

        /// One `Map` node, in print order.
        fn map_node(
            entries: Vec<(StackItem, StackItem)>,
            id: usize,
            stack_references: usize,
            is_read_only: bool,
            indent: usize,
            pretty: bool,
        ) -> Vec<Task> {
            let mut out = Vec::new();
            if pretty {
                out.push(text("Map("));
                out.push(Task::Indent(indent + 4));
                out.push(text("Map {"));
                out.push(Task::Indent(indent + 8));
                out.push(text("inner: Mutex {"));
                out.push(Task::Indent(indent + 12));
                out.push(text("data: MapInner {"));
                out.push(Task::Indent(indent + 16));
                out.push(text("items: "));
                if entries.is_empty() {
                    out.push(text("{}"));
                } else {
                    out.push(text("{"));
                    for (key, value) in entries {
                        out.push(Task::Indent(indent + 20));
                        out.push(Task::Item(key, indent + 20));
                        out.push(text(": "));
                        out.push(Task::Item(value, indent + 20));
                        out.push(text(","));
                    }
                    out.push(Task::Indent(indent + 16));
                    out.push(text("}"));
                }
                out.push(text(","));
                push_num_field(&mut out, "id", id, indent + 16);
                push_num_field(&mut out, "stack_references", stack_references, indent + 16);
                out.push(Task::Indent(indent + 16));
                out.push(text("is_read_only: "));
                out.push(Task::Bool(is_read_only));
                out.push(text(","));
                out.push(Task::Indent(indent + 12));
                out.push(text("},"));
                out.push(Task::Indent(indent + 8));
                out.push(text("},"));
                out.push(Task::Indent(indent + 4));
                out.push(text("},"));
                out.push(Task::Indent(indent));
                out.push(text(")"));
                return out;
            }

            out.push(text("Map(Map { inner: Mutex { data: MapInner { items: {"));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(text(", "));
                }
                out.push(Task::Item(key, 0));
                out.push(text(": "));
                out.push(Task::Item(value, 0));
            }
            out.push(text("}, id: "));
            out.push(Task::Num(id));
            out.push(text(", stack_references: "));
            out.push(Task::Num(stack_references));
            out.push(text(", is_read_only: "));
            out.push(Task::Bool(is_read_only));
            out.push(text(" } } })"));
            out
        }

        let pretty = f.alternate();
        let mut tasks = vec![Task::Item(self.clone(), 0)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Text(value) => f.write_str(&value)?,
                Task::Num(value) => write!(f, "{value}")?,
                Task::Bool(value) => write!(f, "{value}")?,
                Task::Indent(spaces) => {
                    f.write_str("\n")?;
                    for _ in 0..spaces {
                        f.write_str(" ")?;
                    }
                }
                Task::Item(item, indent) => {
                    let tokens = match &item {
                        Self::Array(array) => {
                            let (items, id, stack_references, is_read_only) =
                                array.debug_snapshot();
                            Some(list_node(
                                "Array",
                                items,
                                id,
                                stack_references,
                                is_read_only,
                                indent,
                                pretty,
                            ))
                        }
                        Self::Struct(structure) => {
                            let (items, id, stack_references, is_read_only) =
                                structure.debug_snapshot();
                            Some(list_node(
                                "Struct",
                                items,
                                id,
                                stack_references,
                                is_read_only,
                                indent,
                                pretty,
                            ))
                        }
                        Self::Map(map) => {
                            let (entries, id, stack_references, is_read_only) =
                                map.debug_snapshot();
                            let entries = entries
                                .iter()
                                .map(|(key, value)| (key.clone(), value.clone()))
                                .collect();
                            Some(map_node(
                                entries,
                                id,
                                stack_references,
                                is_read_only,
                                indent,
                                pretty,
                            ))
                        }
                        Self::Null => {
                            f.write_str("Null")?;
                            None
                        }
                        Self::Boolean(value) => {
                            f.write_str(&leaf("Boolean", value, pretty, indent))?;
                            None
                        }
                        Self::Integer(value) => {
                            f.write_str(&leaf("Integer", value, pretty, indent))?;
                            None
                        }
                        Self::ByteString(value) => {
                            f.write_str(&leaf("ByteString", value, pretty, indent))?;
                            None
                        }
                        Self::Buffer(value) => {
                            f.write_str(&leaf("Buffer", value, pretty, indent))?;
                            None
                        }
                        Self::Pointer(value) => {
                            f.write_str(&leaf("Pointer", value, pretty, indent))?;
                            None
                        }
                        Self::InteropInterface(value) => {
                            f.write_str(&leaf("InteropInterface", value, pretty, indent))?;
                            None
                        }
                    };
                    if let Some(tokens) = tokens {
                        tasks.extend(tokens.into_iter().rev());
                    }
                }
            }
        }
        Ok(())
    }
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

    /// Creates a deep clone of the stack item.
    ///
    /// The tree is copied on an explicit frame stack and memoised by compound
    /// identity, so a chain nested as deep as `MaxStackSize` is copied without
    /// adding one native frame per level. Cycles are preserved: a compound
    /// reached a second time resolves to the copy that is still being filled.
    #[must_use]
    pub fn deep_clone(&self) -> Self {
        /// The compound that a frame is filling.
        enum Sink {
            /// An `Array` under construction.
            Array(ArrayItem),
            /// A `Struct` under construction.
            Struct(StructItem),
            /// A `Map` under construction; its sub items arrive as key, value,
            /// key, value, ... because the recursive form copied them in that
            /// order.
            Map {
                map: MapItem,
                pending_key: Option<StackItem>,
            },
        }

        /// One compound being copied and the source items it still needs.
        struct Frame {
            sink: Sink,
            target: StackItem,
            source: std::vec::IntoIter<StackItem>,
        }

        /// Copies an item that has no children of its own.
        fn clone_leaf(item: &StackItem) -> Option<StackItem> {
            match item {
                StackItem::Null => Some(StackItem::Null),
                StackItem::Boolean(value) => Some(StackItem::Boolean(*value)),
                StackItem::Integer(value) => Some(StackItem::Integer(value.clone())),
                StackItem::ByteString(value) => Some(StackItem::ByteString(value.clone())),
                StackItem::Buffer(buffer) => {
                    Some(StackItem::Buffer(BufferItem::new(buffer.data())))
                }
                StackItem::Pointer(pointer) => Some(StackItem::Pointer(pointer.clone())),
                StackItem::InteropInterface(interface) => {
                    Some(StackItem::InteropInterface(interface.clone()))
                }
                _ => None,
            }
        }

        /// Opens the copy of a compound and records it in `refs` before any
        /// child is visited.
        fn open(
            item: &StackItem,
            refs: &mut std::collections::HashMap<CompoundIdentity, StackItem>,
        ) -> Frame {
            match item {
                StackItem::Array(array) => {
                    let copy = ArrayItem::new(Vec::new());
                    let target = StackItem::Array(copy.clone());
                    refs.insert(
                        compound_identity(item).expect("compound item"),
                        target.clone(),
                    );
                    Frame {
                        sink: Sink::Array(copy),
                        target,
                        source: array.items().into_iter(),
                    }
                }
                StackItem::Struct(structure) => {
                    let copy = StructItem::new(Vec::new());
                    let target = StackItem::Struct(copy.clone());
                    refs.insert(
                        compound_identity(item).expect("compound item"),
                        target.clone(),
                    );
                    Frame {
                        sink: Sink::Struct(copy),
                        target,
                        source: structure.items().into_iter(),
                    }
                }
                StackItem::Map(map) => {
                    let copy = MapItem::new(VmOrderedDictionary::new());
                    let target = StackItem::Map(copy.clone());
                    refs.insert(
                        compound_identity(item).expect("compound item"),
                        target.clone(),
                    );
                    let mut source = Vec::with_capacity(map.len() * 2);
                    for (key, value) in map.iter() {
                        source.push(key);
                        source.push(value);
                    }
                    Frame {
                        sink: Sink::Map {
                            map: copy,
                            pending_key: None,
                        },
                        target,
                        source: source.into_iter(),
                    }
                }
                _ => unreachable!("only compound items are opened"),
            }
        }

        /// Stores a finished child copy in the compound being filled. The
        /// recursive form ignored the `push`/`set` results here as well.
        fn accept(sink: &mut Sink, cloned: StackItem) {
            match sink {
                Sink::Array(array) => {
                    let _ = array.push(cloned);
                }
                Sink::Struct(structure) => {
                    let _ = structure.push(cloned);
                }
                Sink::Map { map, pending_key } => match pending_key.take() {
                    None => *pending_key = Some(cloned),
                    Some(key) => {
                        let _ = map.set(key, cloned);
                    }
                },
            }
        }

        let mut refs: std::collections::HashMap<CompoundIdentity, StackItem> =
            std::collections::HashMap::new();
        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some(self.clone());
        let mut cloned = StackItem::Null;

        loop {
            if let Some(item) = pending.take() {
                if let Some(known) =
                    compound_identity(&item).and_then(|identity| refs.get(&identity))
                {
                    cloned = known.clone();
                } else if let Some(leaf) = clone_leaf(&item) {
                    cloned = leaf;
                } else {
                    let mut frame = open(&item, &mut refs);
                    let first = frame.source.next();
                    frames.push(frame);
                    match first {
                        Some(child) => pending = Some(child),
                        None => {
                            let frame = frames.pop().expect("open frame");
                            cloned = frame.target;
                        }
                    }
                    continue;
                }
            }

            if frames.is_empty() {
                return cloned;
            }
            let value = std::mem::replace(&mut cloned, StackItem::Null);
            accept(&mut frames.last_mut().expect("open frame").sink, value);

            let child = frames.last_mut().and_then(|frame| frame.source.next());
            match child {
                Some(child) => pending = Some(child),
                None => {
                    let frame = frames.pop().expect("open frame");
                    cloned = frame.target;
                }
            }
        }
    }

    /// Creates a deep copy respecting execution limits (mirrors C# behaviour).
    pub fn deep_copy(&self, limits: &ExecutionEngineLimits) -> VmResult<Self> {
        match self {
            Self::Struct(structure) => {
                let cloned = structure.clone_with_limits(limits)?;
                Ok(Self::Struct(cloned))
            }
            Self::Array(array) => {
                let copy = array.deep_copy()?;
                Ok(Self::Array(copy))
            }
            Self::Map(map) => {
                let copy = map.deep_copy()?;
                Ok(Self::Map(copy))
            }
            _ => Ok(self.deep_clone()),
        }
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
    ///
    /// Child hashes are collected on a value stack and combined in source order
    /// when their parent finishes, so a deeply nested item does not consume
    /// native stack. A compound that (through a cycle) contains itself
    /// contributes `0` instead of being folded forever.
    #[must_use]
    pub fn hash_code(&self) -> i32 {
        enum Task {
            /// Hash this item.
            Item(StackItem),
            /// Fold the `children` hashes that sit on the top of `hashes`.
            Finish {
                identity: CompoundIdentity,
                children: usize,
            },
        }

        fn leaf_hash(item: &StackItem) -> Option<i32> {
            match item {
                StackItem::Null => Some(0),
                StackItem::Boolean(value) => Some(i32::from(*value)),
                StackItem::Integer(value) => Some(hash_bytes(&value.to_signed_bytes_le())),
                StackItem::ByteString(bytes) => Some(hash_bytes(bytes)),
                StackItem::Buffer(buffer) => Some(buffer.with_data(hash_bytes)),
                StackItem::Pointer(pointer) => {
                    let script_ptr = pointer.script() as *const Script as usize as u64;
                    let mut hash = 17;
                    hash = combine_hash(hash, (script_ptr & 0xFFFF_FFFF) as i32);
                    hash = combine_hash(hash, ((script_ptr >> 32) & 0xFFFF_FFFF) as i32);
                    hash = combine_hash(hash, pointer.position() as i32);
                    Some(hash)
                }
                StackItem::InteropInterface(interface) => {
                    let addr = Arc::as_ptr(interface).cast::<()>() as usize as u64;
                    let mut hash = 17;
                    hash = combine_hash(hash, (addr & 0xFFFF_FFFF) as i32);
                    hash = combine_hash(hash, ((addr >> 32) & 0xFFFF_FFFF) as i32);
                    Some(hash)
                }
                _ => None,
            }
        }

        let mut tasks = vec![Task::Item(self.clone())];
        let mut hashes: Vec<i32> = Vec::new();
        let mut visited = std::collections::HashSet::new();

        while let Some(task) = tasks.pop() {
            match task {
                Task::Item(item) => {
                    if let Some(hash) = leaf_hash(&item) {
                        hashes.push(hash);
                        continue;
                    }
                    let Some(identity) = compound_identity(&item) else {
                        continue;
                    };
                    if !visited.insert(identity) {
                        hashes.push(0);
                        continue;
                    }
                    let children: Vec<StackItem> = match &item {
                        StackItem::Array(array) => array.items(),
                        StackItem::Struct(structure) => structure.items(),
                        StackItem::Map(map) => {
                            let entries = map.items();
                            entries
                                .iter()
                                .flat_map(|(key, value)| [key.clone(), value.clone()])
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    tasks.push(Task::Finish {
                        identity,
                        children: children.len(),
                    });
                    for child in children.into_iter().rev() {
                        tasks.push(Task::Item(child));
                    }
                }
                Task::Finish { identity, children } => {
                    visited.remove(&identity);
                    let start = hashes.len() - children;
                    let mut hash = combine_hash(17, children as i32);
                    for child in &hashes[start..] {
                        hash = combine_hash(hash, *child);
                    }
                    hashes.truncate(start);
                    hashes.push(hash);
                }
            }
        }

        hashes.pop().unwrap_or(0)
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
        let mut budget = LimitBudget {
            remaining_stack: u64::from(limits.max_stack_size),
            remaining_comparable: i64::from(limits.max_comparable_size),
            visited: std::collections::HashSet::new(),
        };
        self.equals_with_budget(other, &mut budget)
    }

    /// Walks the pair on an explicit step stack.
    ///
    /// C# `Struct.Equals(StackItem other, ExecutionLimits limits)` keeps a
    /// `Queue<StackItem>`, spends one `MaxStackSize` unit per compared sub item
    /// and one `MaxComparableSize` unit per non-`ByteString` sub item. The walk
    /// below spends the budget at exactly the same points, so a struct nested
    /// as deep as `MaxStackSize` compares without adding one native frame per
    /// level.
    fn equals_with_budget(&self, other: &Self, budget: &mut LimitBudget) -> VmResult<bool> {
        /// One unit of work, consumed in LIFO order.
        enum Step {
            /// Compare the two items.
            Compare(StackItem, StackItem),
            /// Spend the per-sub-item budget of a pair of struct elements.
            Account(StackItem, StackItem),
            /// Leave a struct pair that was being compared.
            Leave(usize, usize),
        }

        /// Outcome of one comparison, before the stack is updated.
        enum Outcome {
            Equal,
            NotEqual,
            Error(VmError),
            Struct {
                left: Box<StructItem>,
                right: Box<StructItem>,
                key: (usize, usize),
            },
        }

        /// Releases every struct pair that is still being compared.
        fn unwind(steps: &mut [Step], budget: &mut LimitBudget) {
            for step in steps.iter() {
                if let Step::Leave(a_id, b_id) = step {
                    budget.visited.remove(&(*a_id, *b_id));
                }
            }
        }

        /// The element-wise comparison of a struct pair for every item kind
        /// that is not descended into.
        fn inline_compare(left: &StackItem, right: &StackItem) -> bool {
            match (left, right) {
                (StackItem::Buffer(a), StackItem::Buffer(b)) => a.id() == b.id(),
                (StackItem::Array(a), StackItem::Array(b)) => a.id() == b.id(),
                (StackItem::Map(a), StackItem::Map(b)) => a.id() == b.id(),
                (StackItem::Boolean(a), StackItem::Boolean(b)) => a == b,
                (StackItem::Integer(a), StackItem::Integer(b)) => a.to_bigint() == b.to_bigint(),
                (StackItem::Pointer(a), StackItem::Pointer(b)) => a == b,
                (StackItem::InteropInterface(a), StackItem::InteropInterface(b)) => {
                    Arc::ptr_eq(a, b)
                }
                _ => false,
            }
        }

        fn compare_byte_strings(a: &[u8], b: &[u8], remaining: &mut i64) -> VmResult<bool> {
            if *remaining == 0 {
                return Err(VmError::invalid_operation_msg(
                    "The operand exceeds the maximum comparable size",
                ));
            }
            if a.len() as i64 > *remaining {
                return Err(VmError::invalid_operation_msg(format!(
                    "The operand exceeds the maximum comparable size, {}/{}",
                    a.len(),
                    *remaining
                )));
            }
            if b.len() as i64 > *remaining {
                return Err(VmError::invalid_operation_msg(format!(
                    "The operand exceeds the maximum comparable size, {}/{}",
                    b.len(),
                    *remaining
                )));
            }
            let compared = a.len().max(b.len()).max(1) as i64;
            *remaining -= compared;
            Ok(a == b)
        }

        let mut steps = vec![Step::Compare(self.clone(), other.clone())];
        while let Some(step) = steps.pop() {
            match step {
                Step::Leave(a_id, b_id) => {
                    budget.visited.remove(&(a_id, b_id));
                }
                Step::Account(left, right) => {
                    if budget.remaining_stack == 0 {
                        unwind(&mut steps, budget);
                        return Err(VmError::invalid_operation_msg(
                            "Too many struct items to compare in struct comparison",
                        ));
                    }
                    budget.remaining_stack -= 1;
                    if matches!(left, StackItem::ByteString(_)) {
                        // Byte strings spend the comparable budget inside
                        // their own comparison.
                        steps.push(Step::Compare(left, right));
                        continue;
                    }
                    if budget.remaining_comparable == 0 {
                        unwind(&mut steps, budget);
                        return Err(VmError::invalid_operation_msg(
                            "The operand exceeds the maximum comparable size in struct comparison",
                        ));
                    }
                    budget.remaining_comparable -= 1;
                    if matches!(left, StackItem::Struct(_)) {
                        steps.push(Step::Compare(left, right));
                    } else if !inline_compare(&left, &right) {
                        unwind(&mut steps, budget);
                        return Ok(false);
                    }
                }
                Step::Compare(left, right) => {
                    let outcome = match (&left, &right) {
                        (Self::Null, Self::Null) => Outcome::Equal,
                        (Self::Null, _) | (_, Self::Null) => Outcome::NotEqual,
                        (Self::Buffer(a), Self::Buffer(b)) => {
                            if a.id() == b.id() {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::Array(a), Self::Array(b)) => {
                            if a.id() == b.id() {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::Map(a), Self::Map(b)) => {
                            if a.id() == b.id() {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::Struct(a), Self::Struct(b)) => {
                            if a.id() == b.id() {
                                Outcome::Equal
                            } else {
                                let key = (a.id(), b.id());
                                if budget.visited.insert(key) {
                                    Outcome::Struct {
                                        left: Box::new(a.clone()),
                                        right: Box::new(b.clone()),
                                        key,
                                    }
                                } else {
                                    // C# uses ReferenceEquals(a, b) to
                                    // short-circuit cycles inside structs; if
                                    // the same pair is re-entered treat it as
                                    // equal.
                                    Outcome::Equal
                                }
                            }
                        }
                        (Self::Boolean(a), Self::Boolean(b)) => {
                            if a == b {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::Integer(a), Self::Integer(b)) => {
                            if a.to_bigint() == b.to_bigint() {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::ByteString(a), Self::ByteString(b)) => {
                            match compare_byte_strings(a, b, &mut budget.remaining_comparable) {
                                Ok(true) => Outcome::Equal,
                                Ok(false) => Outcome::NotEqual,
                                Err(error) => Outcome::Error(error),
                            }
                        }
                        (Self::Pointer(a), Self::Pointer(b)) => {
                            if a == b {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        (Self::InteropInterface(a), Self::InteropInterface(b)) => {
                            if Arc::ptr_eq(a, b) {
                                Outcome::Equal
                            } else {
                                Outcome::NotEqual
                            }
                        }
                        _ => Outcome::NotEqual,
                    };

                    match outcome {
                        Outcome::Equal => {}
                        Outcome::NotEqual => {
                            unwind(&mut steps, budget);
                            return Ok(false);
                        }
                        Outcome::Error(error) => {
                            unwind(&mut steps, budget);
                            return Err(error);
                        }
                        Outcome::Struct { left, right, key } => {
                            // The pair stays visited until all elements are
                            // compared; `unwind` releases it on an early
                            // return, exactly like the recursive form.
                            steps.push(Step::Leave(key.0, key.1));
                            let left_items = left.items();
                            let right_items = right.items();
                            if left_items.len() != right_items.len() {
                                unwind(&mut steps, budget);
                                return Ok(false);
                            }
                            let mut children = Vec::with_capacity(left_items.len());
                            for (left, right) in left_items.into_iter().zip(right_items) {
                                // `Account` spends the budget and then requests the
                                // comparison of the pair itself.
                                children.push(Step::Account(left, right));
                            }
                            steps.extend(children.into_iter().rev());
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    /// Checks if two stack items are equal with reference tracking to handle cycles.
    ///
    /// The pair is compared on an explicit frame stack, so an array or struct
    /// nested as deep as `MaxStackSize` compares without adding one native
    /// frame per level. The verdict — and the moments at which a pair leaves
    /// the visited set — is the same as the previous recursive form.
    fn equals_with_refs(
        &self,
        other: &Self,
        visited: &mut std::collections::HashSet<(CompoundIdentity, CompoundIdentity)>,
    ) -> VmResult<bool> {
        /// A compound pair whose elements are being compared.
        enum Frame {
            /// An `Array` or `Struct`, compared element by element.
            Sequence {
                left: Vec<StackItem>,
                right: Vec<StackItem>,
                /// Index of the next element to compare.
                index: usize,
                key: Option<(CompoundIdentity, CompoundIdentity)>,
            },
            /// A `Map`, compared entry by entry: for every left entry, search
            /// the right entries for one whose key and value are both equal.
            Map {
                left: Vec<(StackItem, StackItem)>,
                right: Vec<(StackItem, StackItem)>,
                left_index: usize,
                right_index: usize,
                stage: Stage,
                key: Option<(CompoundIdentity, CompoundIdentity)>,
            },
        }

        #[derive(Clone, Copy)]
        enum Stage {
            Key,
            Value,
        }

        /// What the comparison of one pair produced.
        enum Outcome {
            /// Decided without descending. `release` is false where the
            /// recursive form returned early, which skipped removing the pair
            /// from the visited set.
            Decided {
                value: bool,
                release: bool,
                key: Option<(CompoundIdentity, CompoundIdentity)>,
            },
            /// Descend into `frame`, starting with the given pair.
            Descend {
                frame: Box<Frame>,
                first: (StackItem, StackItem),
            },
        }

        /// What an open frame needs next.
        enum Advance {
            /// Compare the given pair.
            Request(StackItem, StackItem),
            /// The frame is done; `release` tells whether its pair leaves the
            /// visited set.
            Done { release: bool },
        }

        fn frame_key(frame: &Frame) -> Option<(CompoundIdentity, CompoundIdentity)> {
            match frame {
                Frame::Sequence { key, .. } | Frame::Map { key, .. } => *key,
            }
        }

        /// Compares one pair. Growth of the visited set is undone by the
        /// caller, at the point the recursive form did it.
        fn compare_pair(
            left: &StackItem,
            right: &StackItem,
            visited: &mut std::collections::HashSet<(CompoundIdentity, CompoundIdentity)>,
        ) -> Outcome {
            let key = match (compound_identity(left), compound_identity(right)) {
                (Some(left_id), Some(right_id)) => {
                    if visited.contains(&(left_id, right_id))
                        || visited.contains(&(right_id, left_id))
                    {
                        return Outcome::Decided {
                            value: true,
                            release: false,
                            key: None,
                        };
                    }
                    let key = (left_id, right_id);
                    visited.insert(key);
                    Some(key)
                }
                _ => None,
            };

            // C# Neo VM PrimitiveType.Equals is TYPE-STRICT: only items of the
            // SAME concrete primitive type (Integer, ByteString, Boolean)
            // compare equal, and only by value within that type. Cross-type
            // comparison (e.g. `Integer(1) == ByteString([0x01])`) returns
            // FALSE in C#, even when the byte representations match.
            let decided = |value: bool| Outcome::Decided {
                value,
                release: true,
                key,
            };
            match (left, right) {
                (StackItem::Null, StackItem::Null) => decided(true),
                // Buffer uses reference equality (compound type in C# Neo VM).
                (StackItem::Buffer(a), StackItem::Buffer(b)) => decided(a.id() == b.id()),
                (StackItem::Buffer(_), _) | (_, StackItem::Buffer(_)) => decided(false),
                (StackItem::Boolean(a), StackItem::Boolean(b)) => decided(a == b),
                (StackItem::Integer(a), StackItem::Integer(b)) => {
                    decided(a.to_bigint() == b.to_bigint())
                }
                (StackItem::ByteString(a), StackItem::ByteString(b)) => decided(a == b),
                (a, b)
                    if matches!(
                        a,
                        StackItem::Boolean(_) | StackItem::Integer(_) | StackItem::ByteString(_)
                    ) && matches!(
                        b,
                        StackItem::Boolean(_) | StackItem::Integer(_) | StackItem::ByteString(_)
                    ) =>
                {
                    decided(false)
                }
                (StackItem::Pointer(a), StackItem::Pointer(b)) => decided(a == b),
                (StackItem::InteropInterface(a), StackItem::InteropInterface(b)) => {
                    decided(Arc::ptr_eq(a, b))
                }
                (StackItem::Array(a), StackItem::Array(b)) => {
                    if a.len() != b.len() {
                        return Outcome::Decided {
                            value: false,
                            release: false,
                            key,
                        };
                    }
                    let left_items = a.items();
                    let right_items = b.items();
                    let Some(first) = left_items
                        .first()
                        .map(|item| (item.clone(), right_items[0].clone()))
                    else {
                        return decided(true);
                    };
                    Outcome::Descend {
                        frame: Box::new(Frame::Sequence {
                            left: left_items,
                            right: right_items,
                            index: 1,
                            key,
                        }),
                        first,
                    }
                }
                (StackItem::Struct(a), StackItem::Struct(b)) => {
                    if a.len() != b.len() {
                        return Outcome::Decided {
                            value: false,
                            release: false,
                            key,
                        };
                    }
                    let left_items = a.items();
                    let right_items = b.items();
                    let Some(first) = left_items
                        .first()
                        .map(|item| (item.clone(), right_items[0].clone()))
                    else {
                        return decided(true);
                    };
                    Outcome::Descend {
                        frame: Box::new(Frame::Sequence {
                            left: left_items,
                            right: right_items,
                            index: 1,
                            key,
                        }),
                        first,
                    }
                }
                (StackItem::Map(a), StackItem::Map(b)) => {
                    if a.len() != b.len() {
                        return Outcome::Decided {
                            value: false,
                            release: false,
                            key,
                        };
                    }
                    let left_entries: Vec<_> = a
                        .items()
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect();
                    let right_entries: Vec<_> = b
                        .items()
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect();
                    let Some(first) = left_entries
                        .first()
                        .map(|(left_key, _)| (left_key.clone(), right_entries[0].0.clone()))
                    else {
                        return decided(true);
                    };
                    Outcome::Descend {
                        frame: Box::new(Frame::Map {
                            left: left_entries,
                            right: right_entries,
                            left_index: 0,
                            right_index: 0,
                            stage: Stage::Key,
                            key,
                        }),
                        first,
                    }
                }
                _ => decided(false),
            }
        }

        /// Consumes the verdict of a child comparison and says what the frame
        /// needs next. A swallowed error becomes `false`, matching the
        /// `unwrap_or(false)` of the recursive map search.
        fn advance(frame: &mut Frame, result: &mut VmResult<bool>) -> Advance {
            match frame {
                Frame::Sequence {
                    left, right, index, ..
                } => {
                    if !matches!(result, Ok(true)) {
                        return Advance::Done { release: false };
                    }
                    if *index >= left.len() {
                        return Advance::Done { release: true };
                    }
                    let pair = (left[*index].clone(), right[*index].clone());
                    *index += 1;
                    Advance::Request(pair.0, pair.1)
                }
                Frame::Map {
                    left,
                    right,
                    left_index,
                    right_index,
                    stage,
                    ..
                } => {
                    if matches!(result, Ok(true)) {
                        match stage {
                            Stage::Key => {
                                *stage = Stage::Value;
                                let left_value = left[*left_index].1.clone();
                                let right_value = right[*right_index].1.clone();
                                return Advance::Request(left_value, right_value);
                            }
                            Stage::Value => {
                                // This left entry found a match.
                                *left_index += 1;
                                if *left_index >= left.len() {
                                    return Advance::Done { release: true };
                                }
                                *right_index = 0;
                            }
                        }
                    } else {
                        // The candidate failed, or its comparison errored and
                        // the error was swallowed: try the next candidate.
                        *result = Ok(false);
                        *right_index += 1;
                        if *right_index >= right.len() {
                            return Advance::Done { release: false };
                        }
                    }
                    *stage = Stage::Key;
                    let left_key = left[*left_index].0.clone();
                    let right_key = right[*right_index].0.clone();
                    Advance::Request(left_key, right_key)
                }
            }
        }

        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some((self.clone(), other.clone()));
        let mut result: VmResult<bool> = Ok(true);

        loop {
            if let Some((left, right)) = pending.take() {
                match compare_pair(&left, &right, visited) {
                    Outcome::Decided {
                        value,
                        release,
                        key,
                    } => {
                        if release && let Some(key) = key {
                            visited.remove(&key);
                        }
                        result = Ok(value);
                    }
                    Outcome::Descend { frame, first } => {
                        frames.push(*frame);
                        pending = Some(first);
                        continue;
                    }
                }
            }

            let Some(frame) = frames.last_mut() else {
                return result;
            };
            match advance(frame, &mut result) {
                Advance::Request(left, right) => pending = Some((left, right)),
                Advance::Done { release } => {
                    let frame = frames.pop().expect("open frame");
                    if release && let Some(key) = frame_key(&frame) {
                        visited.remove(&key);
                    }
                }
            }
        }
    }
}

// Implement PartialEq to allow stack items to be compared and used as keys in collections
impl PartialEq for StackItem {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other).unwrap_or(false)
    }
}

impl Eq for StackItem {}

impl TryFrom<StackValue> for StackItem {
    type Error = VmError;

    fn try_from(value: StackValue) -> VmResult<Self> {
        /// A compound whose elements are being converted.
        enum Frame {
            /// An `Array` or `Struct` under construction; `structure` picks
            /// which one.
            List {
                structure: bool,
                source: std::vec::IntoIter<StackValue>,
                items: Vec<StackItem>,
            },
            /// A `Map` under construction; elements arrive as key, value, ...
            Map {
                source: std::vec::IntoIter<StackValue>,
                entries: VmOrderedDictionary<StackItem, StackItem>,
                pending_key: Option<StackItem>,
            },
        }

        /// Converts an item that has no children of its own.
        fn convert_leaf(value: &StackValue) -> VmResult<Option<StackItem>> {
            match value {
                StackValue::Integer(value) => Ok(Some(StackItem::from_i64(*value))),
                StackValue::BigInteger(bytes) => Ok(Some(StackItem::from_int(
                    BigInt::from_signed_bytes_le(bytes),
                ))),
                StackValue::ByteString(bytes) => {
                    Ok(Some(StackItem::from_byte_string(bytes.clone())))
                }
                StackValue::Buffer(bytes) => Ok(Some(StackItem::from_buffer(bytes.clone()))),
                StackValue::Boolean(value) => Ok(Some(StackItem::from_bool(*value))),
                StackValue::Null => Ok(Some(StackItem::Null)),
                StackValue::Pointer(_) | StackValue::Interop(_) | StackValue::Iterator(_) => {
                    Err(VmError::invalid_operation_msg(format!(
                        "Cannot convert {value:?} into neo-vm StackItem without host runtime identity"
                    )))
                }
                _ => Ok(None),
            }
        }

        /// Opens the frame of a compound value.
        fn open(value: StackValue) -> Frame {
            match value {
                StackValue::Array(items) => Frame::List {
                    structure: false,
                    items: Vec::with_capacity(items.len()),
                    source: items.into_iter(),
                },
                StackValue::Struct(items) => Frame::List {
                    structure: true,
                    items: Vec::with_capacity(items.len()),
                    source: items.into_iter(),
                },
                StackValue::Map(entries) => {
                    let capacity = entries.len();
                    let mut source = Vec::with_capacity(capacity * 2);
                    for (key, value) in entries {
                        source.push(key);
                        source.push(value);
                    }
                    Frame::Map {
                        source: source.into_iter(),
                        entries: VmOrderedDictionary::with_capacity(capacity),
                        pending_key: None,
                    }
                }
                _ => unreachable!("only compound values are opened"),
            }
        }

        fn next_child(frame: &mut Frame) -> Option<StackValue> {
            match frame {
                Frame::List { source, .. } | Frame::Map { source, .. } => source.next(),
            }
        }

        fn accept(frame: &mut Frame, item: StackItem) {
            match frame {
                Frame::List { items, .. } => items.push(item),
                Frame::Map {
                    entries,
                    pending_key,
                    ..
                } => match pending_key.take() {
                    None => *pending_key = Some(item),
                    Some(key) => {
                        let _ = entries.insert(key, item);
                    }
                },
            }
        }

        fn close(frame: Frame) -> StackItem {
            match frame {
                Frame::List {
                    structure, items, ..
                } => {
                    if structure {
                        StackItem::from_struct(items)
                    } else {
                        StackItem::from_array(items)
                    }
                }
                Frame::Map { entries, .. } => StackItem::from_map(entries),
            }
        }

        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some(value);
        let mut converted = StackItem::Null;

        loop {
            if let Some(value) = pending.take() {
                if let Some(item) = convert_leaf(&value)? {
                    converted = item;
                } else {
                    let mut frame = open(value);
                    let first = next_child(&mut frame);
                    frames.push(frame);
                    match first {
                        Some(child) => pending = Some(child),
                        None => {
                            let frame = frames.pop().expect("open frame");
                            converted = close(frame);
                        }
                    }
                    continue;
                }
            }

            if frames.is_empty() {
                return Ok(converted);
            }
            let item = std::mem::replace(&mut converted, StackItem::Null);
            accept(frames.last_mut().expect("open frame"), item);

            let child = frames.last_mut().and_then(next_child);
            match child {
                Some(child) => pending = Some(child),
                None => {
                    let frame = frames.pop().expect("open frame");
                    converted = close(frame);
                }
            }
        }
    }
}

impl TryFrom<StackItem> for StackValue {
    type Error = VmError;

    fn try_from(value: StackItem) -> VmResult<Self> {
        /// A compound whose elements are being converted.
        enum Frame {
            /// An `Array` or `Struct` under construction; `structure` picks
            /// which one.
            List {
                structure: bool,
                source: std::vec::IntoIter<StackItem>,
                items: Vec<StackValue>,
            },
            /// A `Map` under construction; elements arrive as key, value, ...
            Map {
                source: std::vec::IntoIter<StackItem>,
                entries: Vec<(StackValue, StackValue)>,
                pending_key: Option<StackValue>,
            },
        }

        fn convert_leaf(value: &StackItem) -> VmResult<Option<StackValue>> {
            match value {
                StackItem::Null => Ok(Some(StackValue::Null)),
                StackItem::Boolean(value) => Ok(Some(StackValue::Boolean(*value))),
                StackItem::Integer(value) => Ok(Some(match value.to_i64() {
                    Some(value) => StackValue::Integer(value),
                    None => StackValue::BigInteger(value.to_signed_bytes_le()),
                })),
                StackItem::ByteString(bytes) => Ok(Some(StackValue::ByteString(bytes.clone()))),
                StackItem::Buffer(buffer) => Ok(Some(StackValue::Buffer(buffer.data()))),
                StackItem::Pointer(pointer) => {
                    let position = i64::try_from(pointer.position()).map_err(|_| {
                        VmError::overflow("StackItem pointer position does not fit neo-vm-rs i64")
                    })?;
                    Ok(Some(StackValue::Pointer(position)))
                }
                StackItem::InteropInterface(_) => Err(VmError::invalid_operation_msg(
                    "Cannot convert InteropInterface into neo-vm-rs StackValue without a host handle",
                )),
                _ => Ok(None),
            }
        }

        fn open(value: StackItem) -> Frame {
            match value {
                StackItem::Array(array) => {
                    let items = array.items();
                    Frame::List {
                        structure: false,
                        items: Vec::with_capacity(items.len()),
                        source: items.into_iter(),
                    }
                }
                StackItem::Struct(structure) => {
                    let items = structure.items();
                    Frame::List {
                        structure: true,
                        items: Vec::with_capacity(items.len()),
                        source: items.into_iter(),
                    }
                }
                StackItem::Map(map) => {
                    let entries = map.iter();
                    let capacity = entries.len();
                    let mut source = Vec::with_capacity(capacity * 2);
                    for (key, entry) in entries {
                        source.push(key);
                        source.push(entry);
                    }
                    Frame::Map {
                        source: source.into_iter(),
                        entries: Vec::with_capacity(capacity),
                        pending_key: None,
                    }
                }
                _ => unreachable!("only compound items are opened"),
            }
        }

        fn next_child(frame: &mut Frame) -> Option<StackItem> {
            match frame {
                Frame::List { source, .. } | Frame::Map { source, .. } => source.next(),
            }
        }

        fn accept(frame: &mut Frame, item: StackValue) {
            match frame {
                Frame::List { items, .. } => items.push(item),
                Frame::Map {
                    entries,
                    pending_key,
                    ..
                } => match pending_key.take() {
                    None => *pending_key = Some(item),
                    Some(key) => entries.push((key, item)),
                },
            }
        }

        fn close(frame: Frame) -> StackValue {
            match frame {
                Frame::List {
                    structure, items, ..
                } => {
                    if structure {
                        StackValue::Struct(items)
                    } else {
                        StackValue::Array(items)
                    }
                }
                Frame::Map { entries, .. } => StackValue::Map(entries),
            }
        }

        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some(value);
        let mut converted = StackValue::Null;

        loop {
            if let Some(value) = pending.take() {
                if let Some(item) = convert_leaf(&value)? {
                    converted = item;
                } else {
                    let mut frame = open(value);
                    let first = next_child(&mut frame);
                    frames.push(frame);
                    match first {
                        Some(child) => pending = Some(child),
                        None => {
                            let frame = frames.pop().expect("open frame");
                            converted = close(frame);
                        }
                    }
                    continue;
                }
            }

            if frames.is_empty() {
                return Ok(converted);
            }
            let item = std::mem::replace(&mut converted, StackValue::Null);
            accept(frames.last_mut().expect("open frame"), item);

            let child = frames.last_mut().and_then(next_child);
            match child {
                Some(child) => pending = Some(child),
                None => {
                    let frame = frames.pop().expect("open frame");
                    converted = close(frame);
                }
            }
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
