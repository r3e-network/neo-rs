//! Shared StackValue encode/decode helpers for native contracts.
//!
//! Replaces the ~265 lines of duplicated `BinarySerializer::deserialize/
//! serialize_stack_value_with_limits` + `ExecutionEngineLimits::default()`
//! boilerplate found across the 11 native contracts. Every helper produces
//! byte-identical output to the inlined code it replaces — only the error
//! message wording may differ slightly.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. It wraps `neo_serialization`
//! and `neo_vm_rs` primitives that every native contract codec already uses;
//! it must not depend on node startup, RPC transport, or P2P sync.

use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, UInt256};
use neo_serialization::BinarySerializer;
use neo_vm::Interoperable;
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// Deserialises a `BinarySerializer`-encoded `StackValue` from `bytes` using
/// the default `ExecutionEngineLimits`, wrapping any decode error with the
/// supplied `label` (e.g. `"Notary deposit"`, `"Oracle request"`).
///
/// Replaces the repeated pattern (found in 14 sites):
///
/// ```ignore
/// let limits = ExecutionEngineLimits::default();
/// let state = BinarySerializer::deserialize_stack_value_with_limits(
///     bytes,
///     limits.max_item_size as usize,
///     limits.max_stack_size as usize,
/// )
/// .map_err(|e| CoreError::deserialization(format!("LABEL: {e}")))?;
/// ```
pub(crate) fn decode_stack_value(bytes: &[u8], label: &str) -> CoreResult<StackValue> {
    let limits = ExecutionEngineLimits::default();
    BinarySerializer::deserialize_stack_value_with_limits(
        bytes,
        limits.max_item_size as usize,
        limits.max_stack_size as usize,
    )
    .map_err(|e| CoreError::deserialization(format!("{label}: {e}")))
}

/// Serialises an `Interoperable` value to its `BinarySerializer` byte form,
/// wrapping any encode error with the supplied `label`.
///
/// Replaces the repeated pattern (found in 12 sites):
///
/// ```ignore
/// let item = T::new(...).to_stack_value();
/// let bytes = BinarySerializer::serialize_stack_value_default(&item)
///     .map_err(|e| CoreError::serialization(format!("LABEL: {e}")))?;
/// ```
///
/// The `Interoperable` trait's `to_stack_value` delegates to the type's
/// inherent `to_stack_value` method (via `impl_interoperable_via_stack_value!`),
/// so the produced bytes are identical to the inlined code.
pub(crate) fn encode_storage_struct<T: Interoperable>(
    value: &T,
    label: &str,
) -> CoreResult<Vec<u8>> {
    let item = value
        .to_stack_value()
        .map_err(|e| CoreError::serialization(format!("{label}: {e}")))?;
    BinarySerializer::serialize_stack_value_default(&item)
        .map_err(|e| CoreError::serialization(format!("{label}: {e}")))
}

/// Position-based decoder for `StackValue::Struct` items.
///
/// Replaces the repeated `StackValue::Struct(items)` destructure +
/// index-by-position decode pattern found in 8 `from_stack_value` impls
/// (`DepositState`, `NeoAccountStateView`, `CandidateState`, `CachedCommittee`,
/// `WhitelistedContractView`, `HashIndexState`, `OracleRequest`, `AccountState`).
///
/// Each accessor method takes a zero-based position `i` and a human-readable
/// `field` name, producing a labelled `CoreError::invalid_data` on failure.
pub(crate) struct StructDecoder<'a> {
    items: &'a [StackValue],
    label: &'a str,
}

impl<'a> StructDecoder<'a> {
    /// Creates a decoder from a `StackValue` that must be a `Struct`.
    pub fn new(value: &'a StackValue, label: &'a str) -> CoreResult<Self> {
        let StackValue::Struct(_, items) = value else {
            return Err(CoreError::invalid_data(format!("{label} is not a struct")));
        };
        Ok(Self { items, label })
    }

    /// Returns the number of fields in the struct.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if the struct has no fields.
    // Rationale: native-contract decoders keep this C#-parity helper available
    // even when a specific contract path does not currently call it.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns `true` if the field at position `i` is `StackValue::Null`.
    pub fn is_null(&self, i: usize) -> bool {
        self.items
            .get(i)
            .is_some_and(|v| matches!(v, StackValue::Null))
    }

    /// Decodes the field at position `i` as a `BigInt`.
    pub fn bigint(&self, i: usize, field: &str) -> CoreResult<BigInt> {
        let v = self
            .items
            .get(i)
            .ok_or_else(|| CoreError::invalid_data(format!("{} {} missing", self.label, field)))?;
        neo_vm::stack_value_as_bigint(v)
            .map_err(|e| CoreError::invalid_data(format!("{} {}: {e}", self.label, field)))
    }

    /// Decodes the field at position `i` as a `u32`.
    pub fn u32(&self, i: usize, field: &str) -> CoreResult<u32> {
        let v = self.bigint(i, field)?;
        v.to_u32().ok_or_else(|| {
            CoreError::invalid_data(format!("{} {} out of u32 range", self.label, field))
        })
    }

    /// Decodes the field at position `i` as an `i64`.
    pub fn i64(&self, i: usize, field: &str) -> CoreResult<i64> {
        let v = self.bigint(i, field)?;
        v.to_i64().ok_or_else(|| {
            CoreError::invalid_data(format!("{} {} out of i64 range", self.label, field))
        })
    }

    /// Decodes the field at position `i` as an `i32`.
    pub fn i32(&self, i: usize, field: &str) -> CoreResult<i32> {
        let v = self.bigint(i, field)?;
        v.to_i32().ok_or_else(|| {
            CoreError::invalid_data(format!("{} {} out of i32 range", self.label, field))
        })
    }

    /// Decodes the field at position `i` as a `bool`.
    pub fn bool_value(&self, i: usize, field: &str) -> CoreResult<bool> {
        let v = self
            .items
            .get(i)
            .ok_or_else(|| CoreError::invalid_data(format!("{} {} missing", self.label, field)))?;
        neo_vm_rs::stack_value_as_bool(v).ok_or_else(|| {
            CoreError::invalid_data(format!(
                "{} {}: expected boolean-compatible value",
                self.label, field
            ))
        })
    }

    /// Decodes the field at position `i` as a byte vector (`ByteString` or
    /// `Buffer`).
    pub fn byte_array(&self, i: usize, field: &str) -> CoreResult<Vec<u8>> {
        let v = self
            .items
            .get(i)
            .ok_or_else(|| CoreError::invalid_data(format!("{} {} missing", self.label, field)))?;
        v.to_byte_string_bytes().ok_or_else(|| {
            CoreError::invalid_data(format!(
                "{} {}: expected byte-like value",
                self.label, field
            ))
        })
    }

    /// Decodes the field at position `i` as a UTF-8 string.
    pub fn string(&self, i: usize, field: &str) -> CoreResult<String> {
        let bytes = self.byte_array(i, field)?;
        String::from_utf8(bytes)
            .map_err(|e| CoreError::invalid_data(format!("{} {}: {e}", self.label, field)))
    }

    /// Decodes the field at position `i` as an `ECPoint` (compressed public key).
    pub fn ec_point(&self, i: usize, field: &str) -> CoreResult<ECPoint> {
        let bytes = self.byte_array(i, field)?;
        ECPoint::from_bytes(&bytes)
            .map_err(|e| CoreError::invalid_data(format!("{} {}: {e}", self.label, field)))
    }

    /// Decodes the field at position `i` as a `UInt160` (Hash160).
    pub fn hash160(&self, i: usize, field: &str) -> CoreResult<UInt160> {
        let bytes = self.byte_array(i, field)?;
        UInt160::from_bytes(&bytes)
            .map_err(|e| CoreError::invalid_data(format!("{} {}: {e}", self.label, field)))
    }

    /// Decodes the field at position `i` as a `UInt256` (Hash256).
    pub fn hash256(&self, i: usize, field: &str) -> CoreResult<UInt256> {
        let bytes = self.byte_array(i, field)?;
        UInt256::from_bytes(&bytes)
            .map_err(|e| CoreError::invalid_data(format!("{} {}: {e}", self.label, field)))
    }
}
