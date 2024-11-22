use alloc::rc::Rc;
use std::cell::RefCell;
use std::cmp::{max, PartialEq};
use std::fmt;
use std::hash::{Hash, Hasher};
use neo_base::errors;
use neo_base::math::I256;
use neo_type::{Bytes, ScriptHash};
use crate::{StackItemType, MAX_COMPARABLE_SIZE, MAX_INT_SIZE, MAX_STACK_SIZE};
use std::collections::HashMap;
use std::ops::Deref;
use crate::compound_types::array_item::ArrayItem;
use crate::compound_types::compound_trait::CompoundTrait;
use crate::compound_types::map_item::MapItem;
use crate::compound_types::struct_item::StructItem;
use crate::vm_types::interop_item::InteropItem;
use crate::vm_types::pointer_item::PointerItem;
use crate::vm_types::type_error::{CheckedEqError, TypeError};

const MAX_BIGINTEGER_SIZE_BITS: usize = 32 * 8;
const MAX_SIZE: usize = u16::MAX as usize * 2;
const MAX_COMPARABLE_NUM_OF_ITEMS: usize = 2048; // Assuming MaxDeserialized is 2048
const MAX_CLONABLE_NUM_OF_ITEMS: usize = 2048;
const MAX_BYTE_ARRAY_COMPARABLE_SIZE: usize = u16::MAX as usize + 1;
const MAX_KEY_SIZE: usize = 64;

/// Type alias for a thread-safe reference-counted StackItem
pub type SharedItem = Rc<RefCell<StackItem>>;

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

impl StackItem {
    // Type-related methods
    #[inline]
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

    #[inline]
    pub fn primitive_type(&self) -> bool {
        matches!(self, Self::Boolean(_) | Self::Integer(_) | Self::ByteArray(_))
    }

    #[inline]
    pub fn track_reference(&self) -> bool {
        matches!(self, Self::Buffer(_) | Self::Array(_) | Self::Struct(_) | Self::Map(_))
    }

    // Constructors
    #[inline]
    pub fn with_null() -> Self {
        Self::Null
    }

    #[inline]
    pub fn with_boolean(value: bool) -> Self {
        Self::Boolean(value)
    }

    #[inline]
    pub fn with_integer(value: I256) -> Self {
        Self::Integer(value)
    }

    pub fn with_array(items: Vec<StackItem>) -> Self {
        Self::Array(ArrayItem::from_vec(items))
    }

    pub fn with_struct(items: Vec<StackItem>) -> Self {
        Self::Struct(StructItem::from_vec(items))
    }

    pub fn with_map() -> Self {
        Self::Map(MapItem::default())
    }

    // Type conversion methods
    pub fn try_bool(&self) -> Result<bool, TypeError> {
        match self {
            Self::Null => Ok(false),
            Self::Boolean(b) => Ok(*b),
            Self::Integer(i) => Ok(!i.is_zero()),
            Self::ByteArray(b) => {
                if b.len() > MAX_BIGINTEGER_SIZE_BITS / 8 {
                    return Err(TypeError::TooBig);
                }
                Ok(b.iter().any(|&x| x != 0))
            }
            _ => Ok(true),
        }
    }

    pub fn try_bytes(&self) -> Result<Bytes, TypeError> {
        match self {
            Self::ByteArray(b) => Ok(b.clone()),
            Self::Buffer(b) => Ok(Bytes::from(b.clone())),
            Self::Boolean(b) => Ok(Bytes::from(vec![if *b { 1 } else { 0 }])),
            Self::Integer(i) => {
                if i.bits() > MAX_BIGINTEGER_SIZE_BITS {
                    return Err(TypeError::TooBig);
                }
                Ok(Bytes::from(i.to_bytes_be().1))
            }
            _ => Err(TypeError::InvalidConversion),
        }
    }

    pub fn try_integer(&self) -> Result<I256, TypeError> {
        match self {
            Self::Integer(i) => Ok(*i),
            Self::Boolean(b) => Ok(I256::from(if *b { 1 } else { 0 })),
            Self::ByteArray(b) => {
                if b.len() > MAX_BIGINTEGER_SIZE_BITS / 8 {
                    return Err(TypeError::TooBig);
                }
                to_i256(b.as_ref()).map_err(|_| TypeError::InvalidConversion)
            }
            _ => Err(TypeError::InvalidConversion),
        }
    }

    pub fn try_array(&self) -> Result<&ArrayItem, TypeError> {
        match self {
            Self::Array(a) => Ok(a),
            _ => Err(TypeError::InvalidConversion),
        }
    }

    pub fn try_struct(&self) -> Result<&StructItem, TypeError> {
        match self {
            Self::Struct(s) => Ok(s),
            _ => Err(TypeError::InvalidConversion),
        }
    }

    pub fn try_map(&self) -> Result<&MapItem, TypeError> {
        match self {
            Self::Map(m) => Ok(m),
            _ => Err(TypeError::InvalidConversion),
        }
    }

    pub fn convert(&self, target_type: StackItemType) -> Result<StackItem, TypeError> {
        match (self, target_type) {
            (s, t) if s.get_type() == t => Ok(s.clone()),
            (Self::Boolean(b), StackItemType::ByteArray) => {
                Ok(Self::ByteArray(Bytes::from(vec![if *b { 1 } else { 0 }])))
            }
            (Self::Boolean(b), StackItemType::Integer) => {
                Ok(Self::Integer(I256::from(if *b { 1 } else { 0 })))
            }
            _ => Err(TypeError::InvalidConversion),
        }
    }

    // Comparison methods
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
            (Self::Null, Self::Null) => Ok(true),
            (Self::Pointer(l), Self::Pointer(r)) => Ok(l == r),
            (Self::Boolean(l), Self::Boolean(r)) => Ok(l == r),
            (Self::Integer(l), Self::Integer(r)) => Ok(l == r),
            (Self::ByteArray(l), Self::ByteArray(r)) => {
                *limits -= 1.max(l.len().max(r.len()) as isize - 1);
                if *limits < 0 {
                    return Err(CheckedEqError::ExceedMaxComparableSize(StackItemType::ByteArray));
                }
                Ok(l == r)
            }
            (Self::Buffer(l), Self::Buffer(r)) => Ok(core::ptr::eq(l, r)),
            (Self::Array(l), Self::Array(r)) => Ok(l == r),
            (Self::Struct(l), Self::Struct(r)) => {
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
            (Self::Map(l), Self::Map(r)) => Ok(l == r),
            (Self::Interop(l), Self::Interop(r)) => Ok(l == r),
            _ => Ok(false),
        }
    }

    // Cloning and copying
    pub fn deep_copy(&self, as_immutable: bool) -> Self {
        let mut seen = HashMap::new();
        self.deep_copy_internal(&mut seen, as_immutable)
    }

    fn deep_copy_internal(&self, seen: &mut HashMap<*const (), StackItem>, as_immutable: bool) -> Self {
        let ptr = self as *const _ as *const ();
        if let Some(item) = seen.get(&ptr) {
            return item.clone();
        }

        let result = match self {
            Self::Null => Self::Null,
            Self::Boolean(b) => Self::Boolean(*b),
            Self::Integer(i) => Self::Integer(i.clone()),
            Self::ByteArray(b) => Self::ByteArray(b.clone()),
            Self::Buffer(b) => {
                if as_immutable {
                    Self::ByteArray(b.clone().into())
                } else {
                    Self::Buffer(b.clone())
                }
            }
            Self::Array(arr) => {
                let new_items = arr.items().iter()
                    .map(|item| item.deep_copy_internal(seen, as_immutable))
                    .collect();
                Self::with_array(new_items)
            }
            Self::Struct(s) => {
                let new_items = s.items().iter()
                    .map(|item| item.deep_copy_internal(seen, as_immutable))
                    .collect();
                Self::Struct(StructItem { items: new_items, ref_count: 0, read_only: false })
            }
            Self::Map(m) => {
                let mut new_map = MapItem::default();
                for (k, v) in m.items().iter() {
                    let new_key = k.deep_copy_internal(seen, false);
                    let new_val = v.deep_copy_internal(seen, as_immutable);
                    new_map.insert(new_key, new_val);
                }
                Self::Map(new_map)
            }
            Self::Interop(i) => Self::Interop(i.clone()),
            Self::Pointer(p) => Self::Pointer(p.clone()),
        };

        seen.insert(ptr, result.clone());
        result
    }

    // Validation methods
    pub fn is_valid_map_key(&self) -> bool {
        match self {
            Self::Boolean(_) | Self::Integer(_) => true,
            Self::ByteArray(b) => b.len() <= MAX_KEY_SIZE,
            _ => false,
        }
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

impl TryFrom<&StackItem> for &ArrayItem {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        match item {
            StackItem::Array(a) => Ok(a),
            _ => Err(FromTypeError(item.get_type())),
        }
    }
}

impl TryFrom<&StackItem> for &StructItem {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        match item {
            StackItem::Struct(s) => Ok(s),
            _ => Err(FromTypeError(item.get_type())),
        }
    }
}

impl TryFrom<&StackItem> for &MapItem {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        match item {
            StackItem::Map(m) => Ok(m),
            _ => Err(FromTypeError(item.get_type())),
        }
    }
}

impl TryFrom<&StackItem> for &InteropItem {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        match item {
            StackItem::Interop(i) => Ok(i),
            _ => Err(FromTypeError(item.get_type())),
        }
    }
}

impl TryFrom<&StackItem> for &PointerItem {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        match item {
            StackItem::Pointer(p) => Ok(p),
            _ => Err(FromTypeError(item.get_type())),
        }
    }
}

impl TryFrom<&StackItem> for &[u8] {
    type Error = CastError;

    fn try_from(item: &StackItem) -> Result<Self, Self::Error> {
        match item {
            StackItem::Buffer(b) => Ok(b.as_slice()),
            StackItem::ByteArray(b) => Ok(b.as_bytes()),
            _ => Err(FromTypeError(item.get_type())),
        }
    }
}

impl TryFrom<I256> for StackItem {
    type Error = CastError;

    fn try_from(value: I256) -> Result<Self, Self::Error> {
        Ok(Self::Integer(value))
    }
}

impl TryFrom<bool> for StackItem {
    type Error = CastError;

    fn try_from(value: bool) -> Result<Self, Self::Error> {
        Ok(Self::Boolean(value))
    }
}

impl TryFrom<Vec<u8>> for StackItem {
    type Error = CastError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Ok(Self::Buffer(value))
    }
}

impl TryFrom<Bytes> for StackItem {
    type Error = CastError;

    fn try_from(value: Bytes) -> Result<Self, Self::Error> {
        Ok(Self::ByteArray(value))
    }
}

impl<T: Into<StackItem>> TryFrom<Vec<T>> for StackItem {
    type Error = CastError;

    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        Ok(Self::Array(ArrayItem::from_vec(
            value.into_iter().map(|x| x.into()).collect()
        )))
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