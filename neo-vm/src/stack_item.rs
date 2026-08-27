//! Stack item type alias and extension methods.
//!
//! `StackItem` is now a type alias for [`neo_vm_rs::StackValue`], the canonical
//! NeoVM value type from the shared `neo-vm-rs` crate. Extension methods provide
//! ergonomic constructors and conversion helpers that match the old `StackItem` API.

use neo_vm_rs::{StackItemType, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::error::{VmError, VmResult};

/// A NeoVM stack value — re-export of [`neo_vm_rs::StackValue`].
///
/// All value semantics, encoding, and conversion rules are defined in
/// `neo-vm-rs`. This alias exists for backward compatibility with existing
/// `neo_vm::StackItem` usage.
pub type StackItem = StackValue;

/// Extension methods on [`StackItem`] (= [`StackValue`]) that provide the
/// ergonomic API previously offered by the old `StackItem` enum.
pub trait StackItemExt {
    fn true_value() -> Self;
    fn false_value() -> Self;
    fn null() -> Self;
    fn from_bool(value: bool) -> Self;
    fn from_i64(value: i64) -> Self;
    fn from_int<T: Into<BigInt>>(value: T) -> Self;
    fn from_byte_string<T: Into<Vec<u8>>>(value: T) -> Self;
    fn from_buffer<T: Into<Vec<u8>>>(value: T) -> Self;
    fn from_array(items: Vec<Self>) -> Self
    where
        Self: Sized;
    fn from_struct(items: Vec<Self>) -> Self
    where
        Self: Sized;
    fn from_map(entries: Vec<(Self, Self)>) -> Self
    where
        Self: Sized;

    fn is_null(&self) -> bool;
    fn as_bool(&self) -> VmResult<bool>;
    fn as_int(&self) -> VmResult<BigInt>;
    fn into_int(self) -> VmResult<BigInt>;
    fn as_bytes(&self) -> VmResult<Vec<u8>>;
    fn into_bytes(self) -> VmResult<Vec<u8>>;
    fn as_bytes_ref(&self) -> Option<&[u8]>;

    fn stack_item_type(&self) -> StackItemType;
}

impl StackItemExt for StackItem {
    #[inline]
    fn true_value() -> Self {
        Self::Boolean(true)
    }

    #[inline]
    fn false_value() -> Self {
        Self::Boolean(false)
    }

    #[inline]
    fn null() -> Self {
        Self::Null
    }

    #[inline]
    fn from_bool(value: bool) -> Self {
        Self::Boolean(value)
    }

    #[inline]
    fn from_i64(value: i64) -> Self {
        Self::Integer(value)
    }

    #[inline]
    fn from_int<T: Into<BigInt>>(value: T) -> Self {
        let bigint = value.into();
        match bigint.to_i64() {
            Some(small) => Self::Integer(small),
            None => Self::BigInteger(bigint.to_signed_bytes_le()),
        }
    }

    #[inline]
    fn from_byte_string<T: Into<Vec<u8>>>(value: T) -> Self {
        Self::ByteString(value.into())
    }

    #[inline]
    fn from_buffer<T: Into<Vec<u8>>>(value: T) -> Self {
        Self::Buffer(value.into())
    }

    #[inline]
    fn from_array(items: Vec<Self>) -> Self {
        Self::Array(items)
    }

    #[inline]
    fn from_struct(items: Vec<Self>) -> Self {
        Self::Struct(items)
    }

    #[inline]
    fn from_map(entries: Vec<(Self, Self)>) -> Self {
        Self::Map(entries)
    }

    #[inline]
    fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[inline]
    fn as_bool(&self) -> VmResult<bool> {
        Ok(neo_vm_rs::semantics::comparison::boolean_value(self))
    }

    #[inline]
    fn as_int(&self) -> VmResult<BigInt> {
        match self {
            Self::Null => Err(VmError::invalid_type_simple(
                "Cannot convert Null to Integer",
            )),
            Self::Boolean(b) => Ok(BigInt::from(i32::from(*b))),
            Self::Integer(i) => Ok(BigInt::from(*i)),
            Self::BigInteger(bytes) => Ok(BigInt::from_signed_bytes_le(bytes)),
            Self::ByteString(b) => bytes_to_bigint(b),
            Self::Buffer(b) => bytes_to_bigint(b),
            _ => Err(VmError::invalid_type_simple("Cannot convert to Integer")),
        }
    }

    #[inline]
    fn into_int(self) -> VmResult<BigInt> {
        match self {
            Self::Null => Err(VmError::invalid_type_simple(
                "Cannot convert Null to Integer",
            )),
            Self::Boolean(b) => Ok(BigInt::from(i32::from(b))),
            Self::Integer(i) => Ok(BigInt::from(i)),
            Self::BigInteger(bytes) => Ok(BigInt::from_signed_bytes_le(&bytes)),
            Self::ByteString(b) => bytes_to_bigint(&b),
            Self::Buffer(b) => bytes_to_bigint(&b),
            _ => Err(VmError::invalid_type_simple("Cannot convert to Integer")),
        }
    }

    #[inline]
    fn as_bytes(&self) -> VmResult<Vec<u8>> {
        self.to_byte_string_bytes()
            .ok_or_else(|| VmError::invalid_type_simple("Cannot convert to ByteArray"))
    }

    #[inline]
    fn into_bytes(self) -> VmResult<Vec<u8>> {
        self.to_byte_string_bytes()
            .ok_or_else(|| VmError::invalid_type_simple("Cannot convert to ByteArray"))
    }

    #[inline]
    fn as_bytes_ref(&self) -> Option<&[u8]> {
        match self {
            Self::ByteString(b) => Some(b.as_slice()),
            _ => None,
        }
    }

    #[inline]
    fn stack_item_type(&self) -> StackItemType {
        match self {
            Self::Null => StackItemType::Any,
            Self::Boolean(_) => StackItemType::Boolean,
            Self::Integer(_) | Self::BigInteger(_) => StackItemType::Integer,
            Self::ByteString(_) => StackItemType::ByteString,
            Self::Buffer(_) => StackItemType::Buffer,
            Self::Array(_) => StackItemType::Array,
            Self::Struct(_) => StackItemType::Struct,
            Self::Map(_) => StackItemType::Map,
            Self::Pointer(_) => StackItemType::Pointer,
            Self::Interop(_) | Self::Iterator(_) => StackItemType::InteropInterface,
        }
    }
}

/// Convert ByteString/Buffer bytes to BigInt (signed LE two's complement).
fn bytes_to_bigint(b: &[u8]) -> VmResult<BigInt> {
    const MAX_INTEGER_SIZE: usize = 32;
    if b.len() > MAX_INTEGER_SIZE {
        return Err(VmError::invalid_type_simple(
            "Cannot convert to Integer: too many bytes",
        ));
    }
    if b.is_empty() {
        return Ok(BigInt::from(0));
    }
    let is_negative = (b[b.len() - 1] & 0x80) != 0;
    if is_negative {
        let mut bytes_copy = b.to_vec();
        let len = bytes_copy.len();
        bytes_copy[len - 1] &= 0x7F;
        let positive_value = BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes_copy);
        let sign_bit_value = BigInt::from(1) << (len * 8 - 1);
        Ok(-(sign_bit_value - positive_value))
    } else {
        Ok(BigInt::from_bytes_le(num_bigint::Sign::Plus, b))
    }
}
