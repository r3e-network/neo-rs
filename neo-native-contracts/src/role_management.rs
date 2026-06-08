//! RoleManagement native contract (id -8).
//!
//! Implements `getDesignatedByRole` of the C#
//! `Neo.SmartContract.Native.RoleManagement`: the public keys designated for a
//! role (StateValidator / Oracle / NeoFSAlphabetNode / P2PNotary), effective at
//! a given block index. The designation that applies is the most recent one
//! whose designation index is ≤ the queried index — C# performs a backward range
//! seek between `(role, index)` and the `(role)` boundary; this module replicates
//! it with a descending prefix scan. The committee-gated `designateAsRole`
//! writer lives in the runtime, which writes the entries this module reads.

use std::any::Any;
use std::sync::LazyLock;

use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::StorageKey;
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::ROLE_MANAGEMENT_HASH;
use crate::role::Role;
use crate::LedgerContract;

/// Lazily-initialised script-hash handle for the RoleManagement contract.
pub static ROLE_MANAGEMENT_HASH_REF: LazyLock<UInt160> =
    LazyLock::new(|| *ROLE_MANAGEMENT_HASH);

/// The RoleManagement native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct RoleManagement;

impl RoleManagement {
    /// Stable native contract id (matches C# `RoleManagement`).
    pub const ID: i32 = -8;

    /// Construct a new `RoleManagement` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the RoleManagement script hash.
    pub fn script_hash() -> UInt160 {
        *ROLE_MANAGEMENT_HASH_REF
    }

    /// Looks up the public keys designated for `role`, effective at block
    /// `height` (the most recent designation with index ≤ `height`).
    pub fn get_designated_by_role_at(
        &self,
        snapshot: &DataCache,
        role: Role,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        match find_designation_value(snapshot, role.as_byte(), height) {
            Some(value) => decode_node_list(&value),
            None => Ok(Vec::new()),
        }
    }
}

/// Finds the serialized node-list value for `role_byte` effective at `index`:
/// the entry with the greatest designation index that is ≤ `index`.
///
/// Designations are stored under key `(RoleManagement.ID, [role_byte, index_be])`.
/// A `Backward` prefix scan yields them in descending designation-index order, so
/// the first one with `designation_index <= index` is the effective designation —
/// equivalent to C#'s `FindRange((role, index), (role), Backward).FirstOrDefault`.
fn find_designation_value(snapshot: &DataCache, role_byte: u8, index: u32) -> Option<Vec<u8>> {
    let prefix = StorageKey::new(RoleManagement::ID, vec![role_byte]);
    for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Backward) {
        let key_bytes = key.key();
        if key_bytes.len() >= 5 {
            let designation_index =
                u32::from_be_bytes([key_bytes[1], key_bytes[2], key_bytes[3], key_bytes[4]]);
            if designation_index <= index {
                return Some(item.value_bytes().into_owned());
            }
        }
    }
    None
}

/// Decodes a serialized node-list (a `BinarySerializer` array of compressed
/// EC-point byte strings) into `ECPoint`s.
fn decode_node_list(value: &[u8]) -> CoreResult<Vec<ECPoint>> {
    let item = BinarySerializer::deserialize(value, &ExecutionEngineLimits::default(), None)
        .map_err(|e| CoreError::deserialization(format!("RoleManagement node list: {e}")))?;
    let StackItem::Array(array) = item else {
        return Err(CoreError::invalid_data(
            "RoleManagement node list is not an array",
        ));
    };
    let mut points = Vec::new();
    for entry in array.items() {
        let bytes = entry
            .as_bytes()
            .map_err(|e| CoreError::invalid_data(format!("RoleManagement node bytes: {e}")))?;
        points.push(
            ECPoint::from_bytes(&bytes)
                .map_err(|e| CoreError::invalid_data(format!("RoleManagement node EC point: {e}")))?,
        );
    }
    Ok(points)
}

/// Serializes an empty node list (C# returns an empty `ECPoint[]`, not `null`,
/// when no designation exists).
fn empty_node_list() -> CoreResult<Vec<u8>> {
    BinarySerializer::serialize(&StackItem::from_array(Vec::new()), &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("RoleManagement empty list: {e}")))
}

static ROLE_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    vec![NativeMethod::new(
        "getDesignatedByRole".to_string(),
        1 << 15,
        true,
        CallFlags::READ_STATES.bits(),
        vec![ContractParameterType::Integer, ContractParameterType::Integer],
        ContractParameterType::Array,
    )]
});

impl NativeContract for RoleManagement {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *ROLE_MANAGEMENT_HASH_REF
    }

    fn name(&self) -> &str {
        "RoleManagement"
    }

    fn methods(&self) -> &[NativeMethod] {
        &ROLE_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "getDesignatedByRole" => {
                let role_value = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("RoleManagement: missing/invalid role")
                    })?;
                // C# validates the role against the Role enum.
                let role_byte = match role_value {
                    4 | 8 | 16 | 32 => role_value as u8,
                    _ => {
                        return Err(CoreError::invalid_operation(format!(
                            "RoleManagement: role {role_value} is not valid"
                        )))
                    }
                };
                let index = args
                    .get(1)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("RoleManagement: missing/invalid index")
                    })?;

                let snapshot = engine.snapshot_cache();
                // C# throws when index > currentIndex + 1.
                let current = LedgerContract::new().current_index(&snapshot)?;
                if current.saturating_add(1) < index {
                    return Err(CoreError::invalid_operation(format!(
                        "RoleManagement: index {index} exceeds current index + 1 ({})",
                        current.saturating_add(1)
                    )));
                }

                match find_designation_value(&snapshot, role_byte, index) {
                    // The stored value is already the BinarySerializer-encoded
                    // node-list array — exactly the Array return wants.
                    Some(value) => Ok(value),
                    None => empty_node_list(),
                }
            }
            other => Err(CoreError::invalid_operation(format!(
                "RoleManagement method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::{StorageItem, StorageKey};

    fn designation_key(role_byte: u8, index: u32) -> StorageKey {
        let mut key = vec![role_byte];
        key.extend_from_slice(&index.to_be_bytes());
        StorageKey::new(RoleManagement::ID, key)
    }

    fn sample_point() -> ECPoint {
        // A genesis-validator public key (valid compressed EC point).
        ECPoint::from_bytes(
            &hex_to_bytes("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c"),
        )
        .unwrap()
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn native_contract_surface() {
        let c = RoleManagement::new();
        assert_eq!(NativeContract::id(&c), -8);
        assert_eq!(NativeContract::name(&c), "RoleManagement");
        assert_eq!(NativeContract::hash(&c), *ROLE_MANAGEMENT_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["getDesignatedByRole"]);
    }

    #[test]
    fn designation_backward_seek_picks_most_recent() {
        let cache = DataCache::new(false);
        let point = sample_point();

        // No designation yet -> empty.
        assert!(RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::Oracle, 100)
            .unwrap()
            .is_empty());

        // Designate the Oracle role at index 10 (a 1-element node list).
        let list = BinarySerializer::serialize(
            &StackItem::from_array(vec![StackItem::from_byte_string(point.to_bytes())]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        cache.add(
            designation_key(Role::Oracle.as_byte(), 10),
            StorageItem::from_bytes(list),
        );

        // Before the designation -> still empty; at/after -> the designated key.
        assert!(RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::Oracle, 9)
            .unwrap()
            .is_empty());
        let got = RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::Oracle, 25)
            .unwrap();
        assert_eq!(got, vec![point.clone()]);

        // A different role is unaffected.
        assert!(RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::StateValidator, 25)
            .unwrap()
            .is_empty());
    }
}
