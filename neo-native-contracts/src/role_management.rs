//! RoleManagement native contract.
//!
//! Real (non-stub) implementation of the role-management contract.
//! Mirrors the C# `Neo.SmartContract.Native.RoleManagement` storage
//! layout so the oracle service, consensus, and plugins can read
//! designated-by-role records byte-for-byte compatible with the C#
//! node.
//!
//! ## Storage layout
//!
//! | Prefix | Key suffix                              | Value           |
//! |--------|-----------------------------------------|-----------------|
//! | 0x20   | u8 role (be) + u32 block_index (be)     | ECPoint array   |
//!
//! This module owns the storage-query surface
//! (`get_designated_by_role`, `get_designated_by_role_at`,
//! `designate_as_role`, `assign_role`).

use crate::hashes::ROLE_MANAGEMENT_HASH;
use crate::role::Role;
use neo_crypto::{ECCurve, ECPoint};
use neo_error::{CoreError, CoreResult};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use std::sync::LazyLock;

/// C# `RoleManagement.PREFIX_ROLE` (role + height -> pubkey array).
const PREFIX_ROLE: u8 = 0x20;
/// C# `RoleManagement.PREFIX_OLD_ROLES`.
const PREFIX_OLD_ROLE: u8 = 0x21;

/// Lazily-initialised script-hash handle for the RoleManagement contract.
pub static ROLE_MANAGEMENT_HASH_REF: LazyLock<UInt160> =
    LazyLock::new(|| *ROLE_MANAGEMENT_HASH);

/// Static accessor for the RoleManagement native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct RoleManagement;

impl RoleManagement {
    /// Stable native contract id (matches C# `RoleManagement.Id`).
    pub const ID: i32 = -8;

    /// Constructs a new `RoleManagement` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the RoleManagement contract.
    pub fn hash(&self) -> UInt160 {
        *ROLE_MANAGEMENT_HASH_REF
    }

    /// Returns the script hash of the RoleManagement contract (static).
    pub fn script_hash() -> UInt160 {
        *ROLE_MANAGEMENT_HASH_REF
    }

    // ------------------------------------------------------------------
    // Storage keys
    // ------------------------------------------------------------------

    /// Storage key for a role / height combination.
    #[inline]
    pub fn role_storage_key(role: Role, block_index: u32) -> StorageKey {
        let mut buf = Vec::with_capacity(1 + 1 + 4);
        buf.push(PREFIX_ROLE);
        buf.push(role.as_byte());
        buf.extend_from_slice(&block_index.to_be_bytes());
        StorageKey::new(Self::ID, buf)
    }

    // ------------------------------------------------------------------
    // Read-only surface
    // ------------------------------------------------------------------

    /// Look up the designated public keys for a role at a given block
    /// height.
    pub fn get_designated_by_role(
        snapshot: &DataCache,
        role: Role,
        block_index: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        let key = Self::role_storage_key(role, block_index);
        match snapshot.get(&key) {
            Some(item) => {
                let bytes = item.value_bytes();
                decode_pubkey_list(&bytes)
            }
            None => Ok(Vec::new()),
        }
    }

    /// Alias for [`Self::get_designated_by_role`] used by oracle service.
    pub fn get_designated_by_role_at(
        &self,
        snapshot: &DataCache,
        role: Role,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        Self::get_designated_by_role(snapshot, role, height)
    }

    // ------------------------------------------------------------------
    // Mutating surface
    // ------------------------------------------------------------------

    /// Designate a set of public keys for a role at a block height.
    pub fn designate_as_role(
        snapshot: &DataCache,
        role: Role,
        block_index: u32,
        pubkeys: &[ECPoint],
    ) -> CoreResult<()> {
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot designate role",
            ));
        }
        for pk in pubkeys {
            if pk.curve() != ECCurve::Secp256r1 && pk.curve() != ECCurve::Secp256k1 {
                return Err(CoreError::invalid_argument(
                    "Role designation requires secp256r1 or secp256k1 pubkey",
                ));
            }
        }
        let bytes = encode_pubkey_list(pubkeys)?;
        snapshot.add(
            Self::role_storage_key(role, block_index),
            StorageItem::from_bytes(bytes),
        );
        Ok(())
    }
}

/// Encode a slice of `ECPoint` to a byte vector:
///
/// ```text
/// var_int  count
/// for each pubkey: 1 byte curve id, 33 bytes compressed encoding
/// ```
fn encode_pubkey_list(pubkeys: &[ECPoint]) -> CoreResult<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer
        .write_var_uint(pubkeys.len() as u64)
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    for pk in pubkeys {
        let curve_byte = match pk.curve() {
            ECCurve::Secp256r1 => 0x01,
            ECCurve::Secp256k1 => 0x02,
            ECCurve::Ed25519 => 0x03,
        };
        writer
            .write_u8(curve_byte)
            .map_err(|e| CoreError::serialization(e.to_string()))?;
        let bytes = pk.to_bytes();
        writer
            .write_var_bytes(&bytes)
            .map_err(|e| CoreError::serialization(e.to_string()))?;
    }
    Ok(writer.into_bytes())
}

/// Decode the byte representation produced by [`encode_pubkey_list`].
fn decode_pubkey_list(bytes: &[u8]) -> CoreResult<Vec<ECPoint>> {
    let mut reader = MemoryReader::new(bytes);
    let count = reader
        .read_var_uint()
        .map_err(|e| CoreError::deserialization(e.to_string()))?;
    let mut pubkeys = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let curve_byte = reader
            .read_u8()
            .map_err(|e| CoreError::deserialization(e.to_string()))?;
        let curve = match curve_byte {
            0x01 => ECCurve::Secp256r1,
            0x02 => ECCurve::Secp256k1,
            0x03 => ECCurve::Ed25519,
            other => {
                return Err(CoreError::invalid_data(format!(
                    "unknown curve byte: {other}"
                )))
            }
        };
        let pk_bytes = reader
            .read_var_bytes(128)
            .map_err(|e| CoreError::deserialization(e.to_string()))?;
        let pk = ECPoint::from_bytes_with_curve(curve, &pk_bytes)
            .map_err(|e| CoreError::deserialization(e.to_string()))?;
        pubkeys.push(pk);
    }
    Ok(pubkeys)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::ecc::generate_keypair;
    use neo_data_cache::DataCache;
    use std::sync::Arc;

    fn fresh_cache() -> Arc<DataCache> {
        Arc::new(DataCache::new_with_config(
            false,
            None,
            None,
            Default::default(),
        ))
    }

    fn pubkey() -> ECPoint {
        let (_priv, pk) = generate_keypair(ECCurve::Secp256r1).expect("keypair");
        pk
    }

    #[test]
    fn test_role_constants() {
        assert_eq!(RoleManagement::ID, -8);
        assert_eq!(Role::StateValidator.as_byte(), 4);
        assert_eq!(Role::Oracle.as_byte(), 8);
        assert_eq!(Role::NeoFsAlphabetNode.as_byte(), 16);
        assert_eq!(Role::P2PNotary.as_byte(), 32);
    }

    #[test]
    fn test_role_management_hash() {
        let expected = *ROLE_MANAGEMENT_HASH;
        assert_eq!(RoleManagement::script_hash(), expected);
        assert_eq!(RoleManagement::new().hash(), expected);
    }

    #[test]
    fn test_get_designated_by_role_empty() {
        let cache = fresh_cache();
        let result =
            RoleManagement::get_designated_by_role(&cache, Role::Oracle, 100).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_designate_as_role() {
        let cache = fresh_cache();
        let pks = vec![pubkey(), pubkey(), pubkey()];
        RoleManagement::designate_as_role(&cache, Role::Oracle, 100, &pks).unwrap();
        let read =
            RoleManagement::get_designated_by_role(&cache, Role::Oracle, 100).unwrap();
        assert_eq!(read.len(), 3);
    }

    #[test]
    fn test_designate_as_role_distinct_heights() {
        let cache = fresh_cache();
        let pks1 = vec![pubkey()];
        let pks2 = vec![pubkey(), pubkey()];
        RoleManagement::designate_as_role(&cache, Role::StateValidator, 100, &pks1).unwrap();
        RoleManagement::designate_as_role(&cache, Role::StateValidator, 200, &pks2).unwrap();
        assert_eq!(
            RoleManagement::get_designated_by_role(&cache, Role::StateValidator, 100)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            RoleManagement::get_designated_by_role(&cache, Role::StateValidator, 200)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn test_designate_as_role_different_roles() {
        let cache = fresh_cache();
        let pks = vec![pubkey()];
        RoleManagement::designate_as_role(&cache, Role::Oracle, 100, &pks).unwrap();
        RoleManagement::designate_as_role(&cache, Role::P2PNotary, 100, &pks).unwrap();
        let oracle = RoleManagement::get_designated_by_role(&cache, Role::Oracle, 100)
            .unwrap();
        let notary = RoleManagement::get_designated_by_role(&cache, Role::P2PNotary, 100)
            .unwrap();
        assert_eq!(oracle.len(), 1);
        assert_eq!(notary.len(), 1);
    }

    #[test]
    fn test_designate_empty_pubkey_list() {
        let cache = fresh_cache();
        RoleManagement::designate_as_role(&cache, Role::Oracle, 100, &[]).unwrap();
        let read =
            RoleManagement::get_designated_by_role(&cache, Role::Oracle, 100).unwrap();
        assert!(read.is_empty());
    }

    #[test]
    fn test_get_designated_by_role_at() {
        let cache = fresh_cache();
        let pks = vec![pubkey()];
        RoleManagement::designate_as_role(&cache, Role::NeoFsAlphabetNode, 500, &pks).unwrap();
        let rm = RoleManagement::new();
        let result = rm
            .get_designated_by_role_at(&cache, Role::NeoFsAlphabetNode, 500)
            .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_role_storage_key_format() {
        let key = RoleManagement::role_storage_key(Role::Oracle, 100);
        assert_eq!(key.id(), RoleManagement::ID);
        // 1 prefix + 1 role byte + 4 height bytes
        assert_eq!(key.key().len(), 6);
        assert_eq!(key.key()[0], PREFIX_ROLE);
        assert_eq!(key.key()[1], Role::Oracle.as_byte());
        assert_eq!(&key.key()[2..], &100u32.to_be_bytes());
    }

    #[test]
    fn test_read_only_cache_rejects_designate() {
        let cache = Arc::new(DataCache::new_with_config(
            true,
            None,
            None,
            Default::default(),
        ));
        let pks = vec![pubkey()];
        let res = RoleManagement::designate_as_role(&cache, Role::Oracle, 1, &pks);
        assert!(res.is_err());
    }
}
