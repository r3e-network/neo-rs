// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{rc::Rc, vec::Vec};
use core::cell::{Ref, RefCell, RefMut};
use core::hash::{Hash, Hasher};

use hashbrown::hash_map::DefaultHashBuilder;
use neo_base::{errors, math::I256};
use num_enum::TryFromPrimitive;

use crate::{Interop, StackItem::*};

pub const MAX_INTEGER_SIZE: usize = 32;

pub type IndexMap = indexmap::IndexMap<StackItem, StackItem, DefaultHashBuilder>;

#[derive(Debug, Copy, Clone, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
pub enum ItemType {
    Any = 0x00,
    Pointer = 0x10,
    Boolean = 0x20,
    Integer = 0x21,
    ByteString = 0x28,
    Buffer = 0x30,
    Array = 0x40,
    Struct = 0x41,
    Map = 0x48,
    InteropInterface = 0x60,
}

/// Array is a reference type
#[derive(Default, Clone, Eq, PartialEq)]
pub struct Array {
    items: Rc<RefCell<Vec<StackItem>>>,
}

impl Array {
    #[inline]
    pub fn items(&self) -> Ref<'_, Vec<StackItem>> { self.items.borrow() }

    #[inline]
    pub fn items_mut(&self) -> RefMut<'_, Vec<StackItem>> { self.items.borrow_mut() }

    #[inline]
    pub fn strong_count(&self) -> usize { Rc::strong_count(&self.items) }
}

impl Hash for Array {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.items.borrow().iter().for_each(|x| x.hash(state));
    }
}

/// Struct is a value type
#[derive(Default, Clone, Hash, Eq, PartialEq)]
pub struct Struct {
    items: Vec<StackItem>,
}

impl Struct {
    #[inline]
    pub fn items(&self) -> &[StackItem] { &self.items }
}

/// Map is a reference type
#[derive(Default, Clone, Eq, PartialEq)]
pub struct Map {
    items: Rc<RefCell<IndexMap>>,
}

impl Map {
    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        Map { items: Rc::new(RefCell::new(IndexMap::with_capacity_and_hasher(n, <_>::default()))) }
    }

    #[inline]
    pub fn items(&self) -> Ref<'_, IndexMap> { self.items.borrow() }

    #[inline]
    pub fn items_mut(&self) -> RefMut<'_, IndexMap> { self.items.borrow_mut() }

    #[inline]
    pub fn strong_count(&self) -> usize { Rc::strong_count(&self.items) }
}

#[derive(Clone)]
pub enum StackItem {
    Null,
    Boolean(bool),

    // TODO: use same struct to represent U265/I256, like `go-ethereum`
    Integer(I256),
    ByteString(Vec<u8>),
    Buffer(Vec<u8>),

    // TODO: cyclic reference
    Array(Array),
    Struct(Struct),
    Map(Map),
    InteropInterface(Interop),
}

impl StackItem {
    pub fn item_type(&self) -> ItemType {
        match &self {
            Null => ItemType::Any,
            Boolean(_) => ItemType::Boolean,
            Integer(_) => ItemType::Integer,
            ByteString(_) => ItemType::ByteString,
            Buffer(_) => ItemType::Buffer,
            Array(_) => ItemType::Array,
            Struct(_) => ItemType::Struct,
            Map(_) => ItemType::Map,
            InteropInterface(_) => ItemType::InteropInterface,
        }
    }

    #[inline]
    pub fn is_null(&self) -> bool { matches!(self, Self::Null) }

    #[inline]
    pub fn with_null() -> Self { Null }

    #[inline]
    pub fn with_boolean(value: bool) -> Self { Boolean(value) }

    #[inline]
    pub fn with_integer(value: I256) -> Self { Integer(value) }

    #[inline]
    pub fn primitive_type(&self) -> bool { matches!(self, Boolean(_) | Integer(_) | ByteString(_)) }

    #[inline]
    pub fn track_reference(&self) -> bool {
        matches!(self, Buffer(_) | Array(_) | Struct(_) | Map(_)) // why Buffer?
    }
}

impl Default for StackItem {
    #[inline]
    fn default() -> Self { Null }
}

#[derive(Debug, errors::Error)]
pub enum CastError {
    #[error("cast: from {0:?} to {1} invalid: {2}")]
    InvalidCast(ItemType, &'static str, &'static str),
}

impl TryInto<bool> for &StackItem {
    type Error = CastError;

    fn try_into(self) -> Result<bool, Self::Error> {
        match self {
            Null => Ok(false),
            Boolean(v) => Ok(*v),
            Integer(v) => Ok(!v.is_zero()),
            ByteString(v) => {
                if v.len() > MAX_INTEGER_SIZE {
                    Err(CastError::InvalidCast(
                        ItemType::ByteString,
                        "Bool",
                        "exceed MAX_INTEGER_SIZE",
                    ))
                } else {
                    Ok(v.iter().find(|x| **x != 0).is_some())
                }
            }
            _ => Ok(true),
        }
    }
}

impl TryInto<I256> for &StackItem {
    type Error = CastError;

    fn try_into(self) -> Result<I256, Self::Error> {
        match self {
            Boolean(v) => {
                if *v {
                    Ok(I256::ONE)
                } else {
                    Ok(I256::ZERO)
                }
            }
            Integer(v) => Ok(*v),
            // ByteString(_) => {} TODO
            _ => Err(CastError::InvalidCast(self.item_type(), "Int", "cannot cast")),
        }
    }
}

impl PartialEq<Self> for StackItem {
    fn eq(&self, other: &Self) -> bool {
        if core::ptr::eq(self, other) {
            return true;
        }

        match (&self, &other) {
            (Null, Null) => true,
            (Boolean(l), Boolean(r)) => l == r,
            (Integer(l), Integer(r)) => l == r,
            (ByteString(l), ByteString(r)) => l == r,
            (Buffer(l), Buffer(r)) => l == r,
            (Array(l), Array(r)) => l == r,
            (Struct(l), Struct(r)) => l == r,
            (Map(l), Map(r)) => l == r,
            (InteropInterface(l), InteropInterface(r)) => l == r,
            (_, _) => false,
        }
    }
}

impl Eq for StackItem {}

impl Hash for StackItem {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Null => state.write_u8(0),
            Boolean(v) => v.hash(state),
            Integer(v) => v.hash(state),
            ByteString(v) => v.hash(state),
            Buffer(v) => v.hash(state),
            Array(v) => v.hash(state),
            Struct(v) => v.hash(state),
            Map(_v) => state.write_u8(0xfe), // TODO
            InteropInterface(v) => v.hash(state),
        }
    }
}
