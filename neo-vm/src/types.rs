// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{rc::Rc, vec, vec::Vec};
use core::hash::{Hash, Hasher};

use hashbrown::HashMap;
use num_enum::TryFromPrimitive;

use neo_base::math::I256;
use crate::Interop;


#[derive(Debug, Copy, Clone, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
pub enum StackItemType {
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


#[derive(Clone)]
pub enum StackItem {
    Null,
    Boolean(bool),
    Integer(I256),
    ByteString(Vec<u8>),
    Buffer(Vec<u8>),
    Array(Vec<Rc<StackItem>>),
    Struct(Vec<Rc<StackItem>>),

    // TODO: key is a Rc?
    Map(HashMap<StackItem, Rc<StackItem>>),
    InteropInterface(Interop),
}


impl StackItem {
    #[inline]
    pub fn track_reference(&self) -> bool {
        use StackItem::*;
        matches!(self, Buffer(_) | Array(_) | Struct(_) | Map(_))
    }

    pub fn item_type(&self) -> StackItemType {
        use StackItem::*;
        match self {
            Null => StackItemType::Any,
            Boolean(_) => StackItemType::Boolean,
            Integer(_) => StackItemType::Integer,
            ByteString(_) => StackItemType::ByteString,
            Buffer(_) => StackItemType::Buffer,
            Array(_) => StackItemType::Array,
            Struct(_) => StackItemType::Struct,
            Map(_) => StackItemType::Map,
            InteropInterface(_) => StackItemType::InteropInterface,
        }
    }
}

impl Default for StackItem {
    fn default() -> Self { Self::Null }
}

impl PartialEq<Self> for StackItem {
    fn eq(&self, other: &Self) -> bool {
        if core::ptr::eq(self, other) {
            return true;
        }

        use StackItem::*;
        match (self, other) {
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
        use StackItem::*;
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


pub(crate) struct Slots {
    items: Vec<Rc<StackItem>>,
}

impl Slots {
    pub fn new(slots: usize) -> Self {
        Self { items: vec![Default::default(); slots] }
    }

    #[inline]
    pub fn len(&self) -> usize { self.items.len() }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&Rc<StackItem>> { self.items.get(index) }
}


impl core::ops::Index<usize> for Slots {
    type Output = Rc<StackItem>;

    fn index(&self, index: usize) -> &Self::Output { &self.items[index] }
}

impl core::ops::IndexMut<usize> for Slots {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.items[index] }
}
