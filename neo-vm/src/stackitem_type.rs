// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{rc::Rc, vec::Vec};
use core::cell::{Ref, RefCell, RefMut};
use core::hash::{Hash, Hasher};

use hashbrown::hash_map::DefaultHashBuilder;
use neo_base::{errors, math::I256};
use neo_type::H160;
use num_enum::TryFromPrimitive;

use crate::{CastError::*, StackItem::*, *};

pub const MAX_INT_SIZE: usize = 32;

pub type IndexMap = indexmap::IndexMap<StackItem, StackItem, DefaultHashBuilder>;

#[derive(Debug, Copy, Clone, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
pub enum StackItemType {
    Any        = 0x00,
    Pointer    = 0x10,
    Boolean    = 0x20,
    Integer    = 0x21,
    ByteString = 0x28,
    Buffer     = 0x30,
    Array      = 0x40,
    Struct     = 0x41,
    Map        = 0x48,
    InteropInterface = 0x60,
}

impl StackItemType {
    pub fn is_valid(tp: u8) -> bool {
        match tp {
            0x00 | 0x10 | 0x20 | 0x21 | 0x28 | 0x30 | 0x40 | 0x41 | 0x48 | 0x60 => true,
            _ => false,
        }
    }

    pub fn is_primitive(tp: u8) -> bool {
        match tp {
            0x20 | 0x21 | 0x28 => true,
            _ => false,
        }
    }

    pub fn is_compound(tp: u8) -> bool {
        match tp {
            0x40 | 0x41 | 0x48 => true,
            _ => false,
        }
    }
}

/// Array is a reference type
#[derive(Default, Clone)]
pub struct Array {
    items: Rc<RefCell<Vec<StackItem>>>, // TODO: remove RefCell
}

impl Array {
    #[inline]
    pub fn new(initial_size: usize) -> Self {
        Self { items: Rc::new(RefCell::new(vec![Null; initial_size])) }
    }

    #[inline]
    pub fn items(&self) -> Ref<'_, Vec<StackItem>> {
        self.items.borrow()
    }

    #[inline]
    pub fn items_mut(&self) -> RefMut<'_, Vec<StackItem>> {
        self.items.borrow_mut()
    }

    #[inline]
    pub fn strong_count(&self) -> usize {
        Rc::strong_count(&self.items)
    }

    #[inline]
    pub(crate) fn as_ptr(&self) -> *const Vec<StackItem> {
        self.items.as_ptr()
    }
}

impl Eq for Array {}

impl PartialEq for Array {
    // Equal only with same Vec<StackItem>
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.as_ptr(), other.as_ptr())
    }
}

impl Hash for Array {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // self.items.borrow().iter().for_each(|x| x.hash(state));
        self.as_ptr().hash(state);
    }
}

/// Struct is a value type
#[derive(Default, Clone)]
pub struct Struct {
    items: Vec<StackItem>,
}

impl Struct {
    #[inline]
    pub fn items(&self) -> &[StackItem] {
        &self.items
    }

    #[inline]
    pub fn items_mut(&mut self) -> &mut [StackItem] {
        &mut self.items
    }
}

impl Eq for Struct {}

impl PartialEq for Struct {
    // `eq` only with same reference, and cannot be compared in `neo C#`
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self, other)
    }
}

impl Hash for Struct {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self as *const Self).hash(state);
    }
}

/// Map is a reference type
#[derive(Default, Clone)]
pub struct Map {
    items: Rc<RefCell<IndexMap>>, // TODO: remove RefCell
}

impl Map {
    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        Map { items: Rc::new(RefCell::new(IndexMap::with_capacity_and_hasher(n, <_>::default()))) }
    }

    #[inline]
    pub fn items(&self) -> Ref<'_, IndexMap> {
        self.items.borrow()
    }

    #[inline]
    pub fn items_mut(&self) -> RefMut<'_, IndexMap> {
        self.items.borrow_mut()
    }

    #[inline]
    pub fn strong_count(&self) -> usize {
        Rc::strong_count(&self.items)
    }

    #[inline]
    pub(crate) fn as_ptr(&self) -> *const IndexMap {
        self.items.as_ptr()
    }
}

impl Eq for Map {}

impl PartialEq for Map {
    // `eq` only with same IndexMap
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.as_ptr(), other.as_ptr())
    }
}

impl Hash for Map {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ptr().hash(state);
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

#[derive(Clone)]
pub enum StackItem {
    Null,
    Pointer(Pointer),
    Boolean(bool),
    // TODO: use one struct to represent U265/I256, like `go-ethereum`
    Integer(I256),
    ByteString(Vec<u8>),
    Buffer(Vec<u8>),
    Array(Array),
    Struct(Struct),
    Map(Map),
    InteropInterface(Interop),
}

impl Default for StackItem {
    #[inline]
    fn default() -> Self {
        Null
    }
}

impl StackItem {
    pub fn item_type(&self) -> StackItemType {
        match &self {
            Null => StackItemType::Any,
            Pointer(_) => StackItemType::Pointer,
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

    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[inline]
    pub fn with_null() -> Self {
        Null
    }

    #[inline]
    pub fn with_boolean(value: bool) -> Self {
        Boolean(value)
    }

    #[inline]
    pub fn with_integer(value: I256) -> Self {
        Integer(value)
    }

    #[inline]
    pub fn primitive_type(&self) -> bool {
        matches!(self, Boolean(_) | Integer(_) | ByteString(_))
    }

    #[inline]
    pub fn track_reference(&self) -> bool {
        matches!(self, Buffer(_) | Array(_) | Struct(_) | Map(_)) // why Buffer?
    }

    #[inline]
    pub fn as_bytes(&self) -> Result<&[u8], CastError> {
        match self {
            ByteString(v) => Ok(&v),
            Buffer(v) => Ok(v),
            _ => Err(InvalidCast(self.item_type(), "&[u8]", "cannot cast")),
        }
    }

    pub fn as_int(&self) -> Result<I256, CastError> {
        match self {
            Boolean(v) => Ok(if *v { I256::ONE } else { I256::ZERO }),
            Integer(v) => Ok(*v),
            ByteString(v) => to_i256(&v),
            _ => Err(InvalidCast(self.item_type(), "Int", "cannot cast")),
        }
    }

    pub fn as_bool(&self) -> Result<bool, CastError> {
        match self {
            Null => Ok(false),
            Boolean(v) => Ok(*v),
            Integer(v) => Ok(!v.is_zero()),
            ByteString(v) => {
                if v.len() > MAX_INT_SIZE {
                    Err(InvalidCast(StackItemType::ByteString, "Bool", "exceed MaxIntSize"))
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
            return Err(CheckedEqError::ExceedMaxComparableSize(StackItemType::ByteString));
        }
        match (self, other) {
            (Null, Null) => Ok(true),
            (Pointer(l), Pointer(r)) => Ok(l == r),
            (Boolean(l), Boolean(r)) => Ok(l == r),
            (Integer(l), Integer(r)) => Ok(l == r),
            (ByteString(l), ByteString(r)) => {
                *limits -= 1.max(l.len().max(r.len()) as isize - 1);
                if *limits < 0 {
                    return Err(CheckedEqError::ExceedMaxComparableSize(StackItemType::ByteString));
                }
                Ok(l == r)
            }
            (Buffer(l), Buffer(r)) => Ok(core::ptr::eq(l, r)),
            (Array(l), Array(r)) => Ok(l == r),
            (Struct(l), Struct(r)) => {
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
            (Map(l), Map(r)) => Ok(l == r),
            (InteropInterface(l), InteropInterface(r)) => Ok(l == r),
            _ => Ok(false),
        }
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
}

impl CastError {
    #[inline]
    pub fn item_type(&self) -> StackItemType {
        match self {
            InvalidCast(item_type, _, _) => *item_type,
        }
    }
}

pub(crate) fn to_i256(v: &[u8]) -> Result<I256, CastError> {
    let n = v.len();
    if n > MAX_INT_SIZE {
        return Err(InvalidCast(StackItemType::ByteString, "Bool", "exceed MaxIntSize"));
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
            (Null, Null) => true,
            (Pointer(l), Pointer(r)) => l == r,
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
            Pointer(v) => v.hash(state),
            Boolean(v) => v.hash(state),
            Integer(v) => v.hash(state),
            ByteString(v) => v.hash(state),
            Buffer(v) => v.hash(state),
            Array(v) => v.hash(state),
            Struct(v) => v.hash(state),
            Map(v) => v.hash(state),
            InteropInterface(v) => v.hash(state),
        }
    }
}

#[cfg(test)]
mod test {
    use neo_base::math::I256;

    use crate::*;

    #[test]
    fn test_to_i256() {
        let v = to_i256(&[0xffu8]).expect("`to_i256` should be ok");
        assert_eq!(v, (-1).into());

        let v = to_i256(&[0x01]).expect("`to_i256` should be ok");
        assert_eq!(v, I256::ONE);

        let _ = to_i256(&[0x01; 33]).expect_err("long bytes should be error");
    }
}
