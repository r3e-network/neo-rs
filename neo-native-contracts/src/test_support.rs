//! Shared test helpers for the native-contract test modules.
//!
//! Prior to this module, identical helpers (`hex`, `sample_committee`,
//! `deploy_native`, `committee_address`, …) and constants
//! (`CM_PREFIX_CONTRACT`, `NEO_PREFIX_COMMITTEE`,
//! `POLICY_PREFIX_ATTRIBUTE_FEE`) were duplicated 3-6 times across the
//! 11 native-contract test modules. This module is the single home for
//! the canonical versions.
//!
//! Only compiled under `#[cfg(test)]` so it has zero impact on the
//! production binary size or compile time.

use neo_crypto::ECPoint;
use neo_primitives::UInt160;
use neo_serialization::BinarySerializer;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;

/// ContractManagement per-contract storage prefix (C# `ContractManagement.Prefix_Contract`).
pub const CM_PREFIX_CONTRACT: u8 = 8;
/// C# `NeoToken.Prefix_Committee` — the committee cache storage key prefix.
pub const NEO_PREFIX_COMMITTEE: u8 = 14;
/// C# `PolicyContract.Prefix_AttributeFee`.
pub const POLICY_PREFIX_ATTRIBUTE_FEE: u8 = 20;

/// Hex-decodes a string of hex digits into a `Vec<u8>`. Panics on invalid
/// input (caller is responsible for supplying a valid string).
///
/// Mirrors the test-only `hex` helpers that previously appeared 5 times
/// across `gas_token.rs`, `treasury.rs`, `policy_contract.rs`,
/// `neo_token.rs`, `crypto_lib.rs`, and `role_management.rs`.
pub fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Three valid secp256r1 public keys (Neo N3 standby validators) used
/// as a committee fixture.
pub fn sample_committee() -> Vec<ECPoint> {
    [
        "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
        "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
        "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
    ]
    .iter()
    .map(|h| ECPoint::from_bytes(&hex(h)).unwrap())
    .collect()
}

/// The `m = n - (n - 1) / 2` committee multisig address for the sample
/// 3-member committee (2-of-3) — used to construct a `Witness` that
/// `check_committee_witness` accepts.
pub fn committee_address(points: &[ECPoint]) -> UInt160 {
    let script =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            2, points,
        )
        .unwrap();
    UInt160::from_script(&script)
}

/// Stores `state` in `cache` under the `(ContractManagement, [CM_PREFIX_CONTRACT] ++ state.hash)` key,
/// matching the C# `ContractManagement.PutContractState` write path. Lets
/// a native contract's test find its own contract state via the
/// standard `lookup_contract_state` call rather than reimplementing the
/// lookup.
pub fn deploy_native(cache: &DataCache, state: &neo_execution::ContractState) {
    let mut key = vec![CM_PREFIX_CONTRACT];
    key.extend_from_slice(&state.hash.to_bytes());
    cache.add(
        StorageKey::new(crate::ContractManagement::ID, key),
        StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
    );
}

/// Stores a committee cache (Array of `Struct[pubkey, votes]`) under
/// `Prefix_Committee`, mirroring C# `CachedCommittee.ToStackItem`.
pub fn seed_committee(cache: &DataCache, points: &[ECPoint]) {
    let array = StackItem::from_array(
        points
            .iter()
            .map(|p| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(p.to_bytes()),
                    StackItem::from_int(0),
                ])
            })
            .collect::<Vec<_>>(),
    );
    let bytes = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default()).unwrap();
    cache.add(
        StorageKey::new(crate::NeoToken::ID, vec![NEO_PREFIX_COMMITTEE]),
        StorageItem::from_bytes(bytes),
    );
}
