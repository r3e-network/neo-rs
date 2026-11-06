//! Test-only helpers for building native-contract storage fixtures.
//!
//! `RoleManagement::serialize_public_keys` is `pub(crate)` inside `neo-native-contracts`, so it
//! is not reachable through the `smart_contract::native` re-export. These helpers reproduce the
//! exact on-storage encoding so neo-core tests can seed a `RoleManagement` designation without
//! widening the visibility of the authoritative crate.

use neo_vm::{BinarySerializer, ExecutionEngineLimits, StackValue};

/// Serializes public keys into the `RoleManagement` storage representation:
/// a VM array of compressed point byte strings, encoded with the binary serializer.
pub(crate) fn serialize_role_public_keys(public_keys: &[neo_core::ECPoint]) -> Vec<u8> {
    let value = StackValue::Array(
        public_keys
            .iter()
            .map(|pubkey| StackValue::ByteString(pubkey.as_bytes().to_vec()))
            .collect(),
    );
    BinarySerializer::serialize_stack_value(&value, &ExecutionEngineLimits::default())
        .expect("serialize state validators")
}
