use alloc::rc::Rc;
use std::cell::RefCell;
use std::cmp::{max, PartialEq};
use std::fmt;
use std::hash::{Hash, Hasher};
use neo_base::errors;
use neo_base::math::I256;
use neo_type::{Bytes, ScriptHash};
use crate::{StackItemType, MAX_COMPARABLE_SIZE, MAX_INT_SIZE, MAX_STACK_SIZE};
use crate::stack_item::CastError::{FromTypeError, InvalidCast};
use std::collections::HashMap;
use std::ops::Deref;
use crate::compound_types::compound_trait::CompoundTrait;
use crate::vm_types::type_error::TypeError;

const MAX_BIGINTEGER_SIZE_BITS: usize = 32 * 8;
const MAX_SIZE: usize = u16::MAX as usize * 2;
const MAX_COMPARABLE_NUM_OF_ITEMS: usize = 2048; // Assuming MaxDeserialized is 2048
const MAX_CLONABLE_NUM_OF_ITEMS: usize = 2048;
const MAX_BYTE_ARRAY_COMPARABLE_SIZE: usize = u16::MAX as usize + 1;
const MAX_KEY_SIZE: usize = 64;

/// Type alias for a thread-safe reference-counted StackItem
pub type SharedItem = Rc<RefCell<StackItem>>;

/// Array is a reference type that holds a vector of thread-safe StackItems
#[derive(Default, Clone, Debug)]
pub struct ArrayItem {
    items: Vec<SharedItem>,
    ref_count:usize,
    read_only:bool,
}
impl CompoundTrait for ArrayItem {
    fn ref_count(&self) -> usize {
        self.ref_count
    }

    fn ref_inc(&mut self, count:usize) -> usize {
        self.ref_count += count;
        self.ref_count
    }

    fn ref_dec(&mut self, count:usize) -> usize {
        self.ref_count -= count;
        self.ref_count
    }

    fn sub_items(&self) -> Vec<SharedItem> {
        self.items.clone()
    }

    fn read_only(&mut self) {
        self.read_only = true;
    }

    fn clear(&mut self) {
        self.items.clear();
    }
}

impl ArrayItem {
    /// Creates a new Array with the specified initial size, filled with Null values
    #[inline]
    pub fn new(initial_size: usize) -> Self {
        Self { 
            items: vec![Rc::new(RefCell::new(StackItem::Null)); initial_size],
            ref_count: 0,
            read_only: false,   
        }
    }

    /// Creates a new Array from an existing vector of StackItems
    #[inline]
    pub fn from_vec(items: Vec<StackItem>) -> Self {
        Self {
            items: items.into_iter()
                .map(|item| Rc::new(RefCell::new(item)))
                .collect(),
            ref_count: 0,
            read_only: false,   
        }
    }

    /// Returns a reference to the vector of items
    #[inline]
    pub fn items(&self) -> &Vec<SharedItem> {
        &self.items
    }

    /// Returns a mutable reference to the vector of items
    #[inline]
    pub fn items_mut(&mut self) -> &mut Vec<SharedItem> {
        &mut self.items
    }

    /// Returns the raw pointer to the first element
    /// This should only be used for comparison operations
    #[inline]
    pub(crate) fn as_ptr(&self) -> *const SharedItem {
        self.items.as_ptr()
    }
}

impl Eq for ArrayItem {}

impl PartialEq for ArrayItem {
    /// Arrays are equal only if they point to the same underlying vector
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.as_ptr(), other.as_ptr())
    }
}

impl Hash for ArrayItem {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ptr().hash(state);
    }
}

/// Struct is a value type that holds a vector of thread-safe StackItems
#[derive(Default, Clone, Debug)]
pub struct StructItem {
    items: Vec<SharedItem>,
    ref_count:usize,
    read_only:bool,
}

impl CompoundTrait for StructItem {
    fn ref_count(&self) -> usize {
        self.ref_count
    }

    fn ref_inc(&mut self, count:usize) -> usize {
        self.ref_count += count;
        self.ref_count
    }

    fn ref_dec(&mut self, count:usize) -> usize {
        self.ref_count -= count;
        self.ref_count
    }

    fn sub_items(&self) -> Vec<SharedItem> {
        self.items.clone()
    }

    fn read_only(&mut self) {
        self.read_only = true;
    }
    
    fn clear(&mut self) {
        self.items.clear();
    }
}

impl StructItem {
    /// Creates a new Struct from an existing vector of StackItems
    #[inline]
    pub fn from_vec(items: Vec<StackItem>) -> Self {
        Self {
            items: items.into_iter()
                .map(|item| Rc::new(RefCell::new(item)))
                .collect(),
            ref_count: 0,
            read_only: false,   
        }
    }

    /// Returns a reference to the vector of items
    #[inline]
    pub fn items(&self) -> &Vec<SharedItem> {
        &self.items
    }

    /// Returns a mutable reference to the vector of items
    #[inline]
    pub fn items_mut(&mut self) -> &mut Vec<SharedItem> {
        &mut self.items
    }

    /// Returns the raw pointer to the first element
    /// This should only be used for comparison operations
    #[inline]
    pub(crate) fn as_ptr(&self) -> *const SharedItem {
        self.items.as_ptr()
    }
}

impl Eq for StructItem {}

impl PartialEq for StructItem {
    // `eq` only with same reference, and cannot be compared in `neo C#`
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.as_ptr(), other.as_ptr())
    }
}

impl Hash for StructItem {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ptr().hash(state);
    }
}

/// Map is a reference type that holds a thread-safe collection of MapElements
#[derive(Default, Clone, Debug)]
pub struct MapItem {
    items:Vec<MapElement>,
    ref_count:usize,
    read_only:bool,
}

#[derive(Debug, Clone)]
pub struct MapElement {
    key: SharedItem,
    value: SharedItem,
}

impl CompoundTrait for MapItem {
    fn ref_count(&self) -> usize {
        self.ref_count
    }
    
    fn ref_inc(&mut self, count:usize) -> usize {
        self.ref_count += count;
        self.ref_count
    }
    
    fn ref_dec(&mut self, count:usize) -> usize {
        self.ref_count -= count;
        self.ref_count
    }
    
    fn sub_items(&self) -> Vec<SharedItem> {
        let mut items = Vec::with_capacity(self.items.len() * 2);
        // Add all keys first
        for element in &self.items {
            items.push(element.key.clone());
        }
        // Then add all values
        for element in &self.items {
            items.push(element.value.clone());
        }
        items
    }
    
    fn read_only(&mut self) {
        self.read_only = true;
    }
    
    fn clear(&mut self) {
        self.items.clear();
    }
}

impl MapItem {
    /// Creates a new Map with the specified capacity
    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        MapItem { 
            items: Vec::with_capacity(n),
            ref_count: 0,
            read_only: false,   
        }
    }
   
    /// Returns the raw pointer to the underlying RefCell
    /// This should only be used for comparison operations
    #[inline]
    pub(crate) fn as_ptr(&self) -> *const RefCell<Vec<MapElement>> {
        Rc::as_ptr(&self.items)
    }
}

impl Eq for MapItem {}

impl PartialEq for MapItem {
    /// Maps are equal only if they point to the same underlying vector
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items, &other.items)
    }
}

impl Hash for MapItem {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ptr().hash(state);
    }
}

impl MapElement {
    /// Creates a new MapElement with the given key and value
    pub fn new(key: StackItem, value: StackItem) -> Self {
        Self {
            key: Rc::new(RefCell::new(key)),
            value: Rc::new(RefCell::new(value)),
        }
    }

    pub fn with_ref(key: SharedItem, value: SharedItem) -> Self {
        Self { key, value }
    }

    /// Returns a reference to the key
    pub fn key(&self) -> &SharedItem {
        &self.key
    }

    /// Returns a reference to the value
    pub fn value(&self) -> &SharedItem {
        &self.value
    }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Pointer {
    pub offset:      u32,
    pub script_hash: ScriptHash,
}

impl Pointer {
    #[inline]
    pub fn new(offset: u32, script_hash: ScriptHash) -> Self {
        Self { offset, script_hash }
    }
}

#[derive(Debug, Clone)]
pub enum StackItem {
    Null,
    Boolean(bool),
    Integer(I256),
    ByteArray(Bytes),
    Buffer(Vec<u8>),
    Array(ArrayItem),
    Struct(StructItem),
    Map(MapItem),
    Interop(InteropItem),
    Pointer(PointerItem),
}

#[derive(Debug, Clone)]
pub struct InteropItem {
    value: Box<dyn std::any::Any>,
}

impl InteropItem {
    pub fn new(value: Box<dyn std::any::Any>) -> Self {
        Self { value }
    }
}

impl PartialEq for &InteropItem {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(*self, *other) {
            return true;
        }

        // Try to downcast both values to Any + Equatable
        let a = self.value.lock().unwrap();
        let b = other.value.lock().unwrap();

        // Check if both can be cast to Equatable
        let a_eq = a.downcast_ref::<Box<dyn Equatable>>();
        let b_eq = b.downcast_ref::<Box<dyn Equatable>>();

        match (a_eq, b_eq) {
            (Some(a), Some(b)) => a.equals(b),
            (None, None) => std::ptr::eq(a.deref(), b.deref()),
            _ => false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerItem {
    pos: usize,
    script: Bytes,
    hash: [u8; 20], // Uint160 equivalent
}


impl Default for StackItem {
    #[inline]
    fn default() -> Self {
        StackItem::Null
    }
}


impl StackItem {
    pub fn get_type(&self) -> StackItemType {
        match self {
            StackItem::Null => StackItemType::Any,
            StackItem::Boolean(_) => StackItemType::Boolean,
            StackItem::Integer(_) => StackItemType::Integer,
            StackItem::ByteArray(_) => StackItemType::ByteArray,
            StackItem::Buffer(_) => StackItemType::Buffer,
            StackItem::Array(_) => StackItemType::Array,
            StackItem::Struct(_) => StackItemType::Struct,
            StackItem::Map(_) => StackItemType::Map,
            StackItem::Interop(_) => StackItemType::InteropInterface,
            StackItem::Pointer(_) => StackItemType::Pointer,
        }
    }

    pub fn try_bool(&self) -> Result<bool, TypeError> {
        match self {
            StackItem::Null => Ok(false),
            StackItem::Boolean(b) => Ok(*b),
            StackItem::Integer(i) => Ok(!i.is_zero()),
            StackItem::ByteArray(b) => {
                if b.len() > MAX_BIGINTEGER_SIZE_BITS / 8 {
                    return Err(TypeError::TooBig);
                }
                Ok(b.iter().any(|&x| x != 0))
            }
            StackItem::Buffer(_) | StackItem::Array(_) | StackItem::Struct(_) |
            StackItem::Map(_) | StackItem::Interop(_) | StackItem::Pointer(_) => Ok(true),
        }
    }

    pub fn try_bytes(&self) -> Result<Bytes, TypeError> {
        match self {
            StackItem::ByteArray(b) => Ok(b.clone()),
            StackItem::Buffer(b) => Ok(Bytes::from(b.clone())),
            StackItem::Boolean(b) => Ok(Bytes::from(vec![if *b { 1 } else { 0 }])),
            StackItem::Integer(i) => {
                if i.bits() > MAX_BIGINTEGER_SIZE_BITS {
                    return Err(TypeError::TooBig);
                }
                Ok(Bytes::from(i.to_bytes_be().1))
            }
            _ => Err(TypeError::InvalidConversion),
        }
    }

    pub fn equals(&self, other: &StackItem) -> bool {
        match (self, other) {
            (StackItem::Null, StackItem::Null) => true,
            (StackItem::Boolean(a), StackItem::Boolean(b)) => a == b,
            (StackItem::Integer(a), StackItem::Integer(b)) => a == b,
            (StackItem::ByteArray(a), StackItem::ByteArray(b)) => {
                let mut limit = MAX_BYTE_ARRAY_COMPARABLE_SIZE;
                Self::equals_byte_array(a, b, &mut limit)
            }
            (StackItem::Array(a), StackItem::Array(b)) => std::ptr::eq(a, b),
            (StackItem::Struct(a), StackItem::Struct(b)) => {
                let mut limit = MAX_COMPARABLE_NUM_OF_ITEMS - 1;
                Self::equals_struct(a, b, &mut limit)
            }
            _ => false,
        }
    }

    fn equals_byte_array(a: &Bytes, b: &Bytes, limit: &mut usize) -> bool {
        if a.len() > *limit || b.len() > *limit {
            panic!("Too big to compare");
        }
        *limit -= max(a.len(), b.len());
        a == b
    }

    fn equals_struct(a: &StructItem, b: &StructItem, limit: &mut usize) -> bool {
        if a.items().len() != b.items().len() {
            return false;
        }

        let mut comparable_size = MAX_BYTE_ARRAY_COMPARABLE_SIZE;
        for (item_a, item_b) in a.iter().zip(b.iter()) {
            *limit -= 1;
            if *limit == 0 {
                panic!("Too many elements");
            }

            match (item_a, item_b) {
                (StackItem::ByteArray(ba_a), StackItem::ByteArray(ba_b)) => {
                    if !Self::equals_byte_array(ba_a, ba_b, &mut comparable_size) {
                        return false;
                    }
                }
                _ => {
                    if comparable_size == 0 {
                        panic!("Too big to compare");
                    }
                    comparable_size -= 1;
                    if !item_a.equals(item_b) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl fmt::Display for StackItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StackItem::Null => write!(f, "Null"),
            StackItem::Boolean(_) => write!(f, "Boolean"),
            StackItem::Integer(_) => write!(f, "BigInteger"),
            StackItem::ByteArray(_) => write!(f, "ByteString"),
            StackItem::Buffer(_) => write!(f, "Buffer"),
            StackItem::Array(_) => write!(f, "Array"),
            StackItem::Struct(_) => write!(f, "Struct"),
            StackItem::Map(_) => write!(f, "Map"),
            StackItem::Interop(_) => write!(f, "InteropInterface"),
            StackItem::Pointer(_) => write!(f, "Pointer"),
        }
    }
}

pub trait Equatable: 'static {
    fn equals(&self, other: &dyn Equatable) -> bool;
}

// Implementation for Map-specific functionality
impl StackItem {
    pub fn is_valid_map_key(&self) -> bool {
        match self {
            StackItem::Boolean(_) | StackItem::Integer(_) => true,
            StackItem::ByteArray(b) => b.len() <= MAX_KEY_SIZE,
            _ => false,
        }
    }
}

// Helper functions for type conversion
impl StackItem {
    pub fn convert(&self, target_type: StackItemType) -> Result<StackItem, TypeError> {
        match (self, target_type) {
            (s, t) if s.get_type() == t => Ok(s.clone()),
            (StackItem::Boolean(b), StackItemType::ByteArray) => {
                Ok(StackItem::ByteArray(Bytes::from(vec![if *b { 1 } else { 0 }])))
            }
            (StackItem::Boolean(b), StackItemType::Integer) => {
                Ok(StackItem::Integer(I256::from(if *b { 1 } else { 0 })))
            }
            // Add more conversion cases as needed
            _ => Err(TypeError::InvalidConversion),
        }
    }
}

impl StackItem {

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[inline]
    pub fn with_null() -> Self {
        StackItem::Null
    }

    #[inline]
    pub fn with_boolean(value: bool) -> Self {
        StackItem::Boolean(value)
    }

    #[inline]
    pub fn with_integer(value: I256) -> Self {
        StackItem::Integer(value)
    }

    pub fn with_array(items: Vec<StackItem>) -> Self {
        StackItem::Array(ArrayItem::from_vec(items))
    }

    pub fn with_struct(items: Vec<StackItem>) -> Self {
        StackItem::Struct(StructItem::from_vec(items) )
    }

    #[inline]
    pub fn primitive_type(&self) -> bool {
        matches!(self, StackItem::Boolean(_) | StackItem::Integer(_) | StackItem::ByteArray(_))
    }

    #[inline]
    pub fn track_reference(&self) -> bool {
        matches!(self, StackItem::Buffer(_) | StackItem::Array(_) | StackItem::Struct(_) | StackItem::Map(_)) // why Buffer?
    }

    #[inline]
    pub fn as_bytes(&self) -> Result<&[u8], CastError> {
        match self {
            StackItem::ByteArray(v) => Ok(v.as_ref()),
            StackItem::Buffer(v) => Ok(v),
            _ => Err(InvalidCast(self.item_type(), "&[u8]", "cannot cast")),
        }
    }

    pub fn as_int(&self) -> Result<I256, CastError> {
        match self {
            StackItem::Boolean(v) => Ok(if *v { I256::ONE } else { I256::ZERO }),
            StackItem::Integer(v) => Ok(*v),
            StackItem::ByteArray(v) => to_i256(v.as_ref()),
            _ => Err(InvalidCast(self.item_type(), "Int", "cannot cast")),
        }
    }

    pub fn as_bool(&self) -> Result<bool, CastError> {
        match self {
            StackItem::Null => Ok(false),
            StackItem::Boolean(v) => Ok(*v),
            StackItem::Integer(v) => Ok(!v.is_zero()),
            StackItem::ByteArray(v) => {
                if v.len() > MAX_INT_SIZE {
                    Err(InvalidCast(StackItemType::ByteArray, "Bool", "exceed MaxIntSize"))
                } else {
                    Ok(v.iter().find(|&&x| x != 0).is_some())
                }
            }
            _ => Ok(true),
        }
    }

    pub fn checked_eq(&self, other: &Self) -> Result<bool, CheckedEqError> {
        let mut limits = MAX_COMPARABLE_SIZE as isize;
        self.recursive_checked_eq(other, &mut limits, 0)
    }

    fn recursive_checked_eq(
        &self,
        other: &Self,
        limits: &mut isize,
        depth: usize,
    ) -> Result<bool, CheckedEqError> {
        if depth > MAX_STACK_SIZE {
            return Err(CheckedEqError::ExceedMaxNestLimit(depth));
        }

        *limits -= 1;
        if *limits < 0 {
            return Err(CheckedEqError::ExceedMaxComparableSize(StackItemType::ByteArray));
        }
        match (self, other) {
            (StackItem::Null, Null) => Ok(true),
            (StackItem::Pointer(l), StackItem::Pointer(r)) => Ok(l == r),
            (StackItem::Boolean(l), StackItem::Boolean(r)) => Ok(l == r),
            (StackItem::Integer(l), StackItem::Integer(r)) => Ok(l == r),
            (StackItem::ByteArray(l), StackItem::ByteArray(r)) => {
                *limits -= 1.max(l.len().max(r.len()) as isize - 1);
                if *limits < 0 {
                    return Err(CheckedEqError::ExceedMaxComparableSize(StackItemType::ByteArray));
                }
                Ok(l == r)
            }
            (StackItem::Buffer(l), StackItem::Buffer(r)) => Ok(core::ptr::eq(l, r)),
            (StackItem::Array(l), StackItem::Array(r)) => Ok(l == r),
            (StackItem::Struct(l), StackItem::Struct(r)) => {
                if l.items().len() != r.items().len() {
                    return Ok(false);
                }

                if *limits - (l.items().len() as isize) < 0 {
                    return Err(CheckedEqError::ExceedMaxComparableSize(StackItemType::Struct));
                }

                for (lz, rz) in l.items().iter().zip(r.items().iter()) {
                    if !lz.recursive_checked_eq(rz, limits, depth + 1)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (StackItem::Map(l), StackItem::Map(r)) => Ok(l == r),
            ( StackItem::Interop(l),  StackItem::Interop(r)) => Ok(l == r),
            _ => Ok(false),
        }
    }


    pub fn successors(&self) -> Vec<SharedItem> {
        match self {
            StackItem::Array(items) => items.items().iter().map(|item| item.clone()).collect(),
            StackItem::Struct(items) => items.items().iter().map(|item| item.clone()).collect(),
            StackItem::Map(items) => {
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for item in items.iter() {
                    keys.push(item.key.clone());
                    values.push(item.value.clone());
                }
                [keys, values].concat()
            },
            _ => vec![],
        }
    }

    pub fn successors_count(&self) -> usize {
        self.successors().len()
    }

}

#[derive(Debug, errors::Error)]
pub enum CheckedEqError {
    #[error("checked_eq: {0:?} exceed max comparable size")]
    ExceedMaxComparableSize(StackItemType),

    #[error("checked_eq: exceed max nest limit: {0}")]
    ExceedMaxNestLimit(usize),
}

#[derive(Debug, errors::Error)]
pub enum CastError {
    #[error("cast: from {0:?} to {1} invalid: {2}")]
    InvalidCast(StackItemType, &'static str, &'static str),
    
    #[error("from TypeError: {0}")]
    FromTypeError(#[from] TypeError),
}

impl CastError {
    #[inline]
    pub fn item_type(&self) -> StackItemType {
        match self {
            InvalidCast(item_type, _, _) => *item_type,
            FromTypeError(err) => err.item_type(),
        }
    }
}

pub(crate) fn to_i256(v: &[u8]) -> Result<I256, CastError> {
    let n = v.len();
    if n > MAX_INT_SIZE {
        return Err(InvalidCast(StackItemType::ByteArray, "Bool", "exceed MaxIntSize"));
    }

    let mut buf = if v.last().map(|&b| (b as i8) < 0).unwrap_or(false) {
        [0xff; MAX_INT_SIZE] // positive
    } else {
        [0x00; MAX_INT_SIZE] // negative
    };
    buf[..n].copy_from_slice(v);

    Ok(I256::from_le_bytes(buf))
}

impl PartialEq<Self> for StackItem {
    fn eq(&self, other: &Self) -> bool {
        if core::ptr::eq(self, other) {
            return true;
        }

        match (&self, &other) {
            (StackItem::Null, StackItem::Null) => true,
            (StackItem::Pointer(l), StackItem::Pointer(r)) => l == r,
            (StackItem::Boolean(l), StackItem::Boolean(r)) => l == r,
            (StackItem::Integer(l), StackItem::Integer(r)) => l == r,
            (StackItem::ByteArray(l), StackItem::ByteArray(r)) => l == r,
            (StackItem::Buffer(l), StackItem::Buffer(r)) => l == r,
            (StackItem::Array(l), StackItem::Array(r)) => l == r,
            (StackItem::Struct(l), StackItem::Struct(r)) => l == r,
            (StackItem::Map(l), StackItem::Map(r)) => l == r,
            (StackItem::Interop(l), StackItem::Interop(r)) => l == r,
            (_, _) => false,
        }
    }
}

impl Eq for StackItem {}

impl Hash for StackItem {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            StackItem::Null => state.write_u8(0),
            StackItem::Pointer(v) => v.hash(state),
            StackItem::Boolean(v) => v.hash(state),
            StackItem::Integer(v) => v.hash(state),
            StackItem::ByteArray(v) => v.hash(state),
            StackItem::Buffer(v) => v.hash(state),
            StackItem::Array(v) => v.hash(state),
            StackItem::Struct(v) => v.hash(state),
            StackItem::Map(v) => v.hash(state),
            StackItem::Interop(v) => v.hash(state),
        }
    }
}

impl StackItem {
    pub fn with_map() -> Self {
        StackItem::Map(MapItem::default())
    }

    pub fn deep_copy(&self, as_immutable: bool) -> Self {
        let mut seen = HashMap::new();
        self.deep_copy_internal(&mut seen, as_immutable)
    }

    fn deep_copy_internal(&self, seen: &mut HashMap<*const (), StackItem>, as_immutable: bool) -> Self {
        // Check if we've seen this item before
        let ptr = self as *const _ as *const ();
        if let Some(item) = seen.get(&ptr) {
            return item.clone();
        }

        let result = match self {
            StackItem::Null => StackItem::Null,
            StackItem::Boolean(b) => StackItem::Boolean(*b),
            StackItem::Integer(i) => StackItem::Integer(i.clone()),
            StackItem::ByteArray(b) => StackItem::ByteArray(b.clone()),
            StackItem::Buffer(b) => {
                if as_immutable {
                    StackItem::ByteArray(b.clone().into())
                } else {
                    StackItem::Buffer(b.clone())
                }
            }
            StackItem::Array(arr) => {
                let new_items = arr.items().iter()
                    .map(|item| item.deep_copy_internal(seen, as_immutable))
                    .collect();
                StackItem::with_array(new_items)
            }
            StackItem::Struct(s) => {
                let new_items = s.items().iter()
                    .map(|item| item.deep_copy_internal(seen, as_immutable))
                    .collect();
                StackItem::Struct(StructItem { items: new_items, ref_count: 0, read_only: false })
            }
            StackItem::Map(m) => {
                let mut new_map = MapItem::default();
                for (k, v) in m.items().iter() {
                    let new_key = k.deep_copy_internal(seen, false); // Keys are always primitive
                    let new_val = v.deep_copy_internal(seen, as_immutable);
                    new_map.items_mut().insert(new_key, new_val);
                }
                StackItem::Map(new_map)
            }
            StackItem::Interop(i) => StackItem::Interop(i.clone()),
            StackItem::Pointer(p) => StackItem::Pointer(p.clone()),
        };

        seen.insert(ptr, result.clone());
        result
    }
}

impl TryFrom<&StackItem> for bool {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        item.as_bool()
    }
}

impl TryFrom<&StackItem> for Vec<u8> {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        item.try_bytes().map_err(CastError::from).map(|b| b.as_bytes().into())
    }
}

impl TryFrom<&StackItem> for I256 {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        item.as_int()
    }
}

#[cfg(test)]
mod test {
    use neo_base::math::I256;

    use crate::*;
    use crate::stack_item::to_i256;

    #[test]
    fn test_to_i256() {
        let v = to_i256(&[0xffu8]).expect("`to_i256` should be ok");
        assert_eq!(v, (-1).into());

        let v = to_i256(&[0x01]).expect("`to_i256` should be ok");
        assert_eq!(v, I256::ONE);

        let _ = to_i256(&[0x01; 33]).expect_err("long bytes should be error");
    }
}

impl StackItem {
    // Convert StackItem to SharedItem
    #[inline]
    pub fn into_shared_item(self) -> SharedItem {
        Rc::new(RefCell::new(self))
    }

    // Get a clone of StackItem from SharedItem
    #[inline]
    pub fn from_shared_item(shared_item: &SharedItem) -> Result<Self, std::cell::BorrowMutError> {
        Ok(shared_item.borrow_mut().clone())
    }

    // Try to get a reference to StackItem from SharedItem
    #[inline]
    pub fn try_borrow(shared_item: &SharedItem) -> Result<std::cell::RefMut<'_, Self>, std::cell::BorrowMutError> {
        shared_item.try_borrow_mut()
    }
}