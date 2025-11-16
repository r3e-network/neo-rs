//! Role Management native contract implementation.
//!
//! Mirrors the behaviour of `Neo.SmartContract.Native.RoleManagement` by
//! persisting designated nodes per role and enforcing committee authorization.

use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::neo_config::SECONDS_PER_BLOCK;
use crate::persistence::{DataCache, IReadOnlyStoreGeneric, SeekDirection};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::{LedgerContract, NativeContract, NativeMethod};
use crate::smart_contract::storage_key::StorageKey;
use crate::{ECPoint, UInt160};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

/// Designated roles in the Neo blockchain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Role {
    /// State validator role.
    StateValidator = 4,
    /// Oracle node role.
    Oracle = 8,
    /// Neo FS Alphabet node role.
    NeoFSAlphabetNode = 16,
    /// P2P notary role.
    P2PNotary = 32,
}

impl Role {
    /// Gets all available roles.
    pub fn all() -> Vec<Role> {
        vec![
            Role::StateValidator,
            Role::Oracle,
            Role::NeoFSAlphabetNode,
            Role::P2PNotary,
        ]
    }

    /// Converts from u8 to Role.
    pub fn from_u8(value: u8) -> Option<Role> {
        match value {
            4 => Some(Role::StateValidator),
            8 => Some(Role::Oracle),
            16 => Some(Role::NeoFSAlphabetNode),
            32 => Some(Role::P2PNotary),
            _ => None,
        }
    }
}

/// The RoleManagement native contract.
pub struct RoleManagement {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl RoleManagement {
    const ID: i32 = -8;
    const MAX_NODES: usize = 32;

    /// Creates a new RoleManagement contract.
    pub fn new() -> Self {
        // RoleManagement contract hash: 0x49cf4e5378ffcd4dec034fd98a174c5491e395e2
        let hash = UInt160::from_bytes(&[
            0x49, 0xcf, 0x4e, 0x53, 0x78, 0xff, 0xcd, 0x4d, 0xec, 0x03, 0x4f, 0xd9, 0x8a, 0x17,
            0x4c, 0x54, 0x91, 0xe3, 0x95, 0xe2,
        ])
        .expect("Operation failed");

        let methods = vec![
            NativeMethod::safe("getDesignatedByRole".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::unsafe_method(
                "designateAsRole".to_string(),
                1 << SECONDS_PER_BLOCK,
                0x01,
            ),
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "getDesignatedByRole" => self.get_designated_by_role(engine, args),
            "designateAsRole" => self.designate_as_role(engine, args),
            _ => Err(Error::native_contract(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    fn get_designated_by_role(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        let (role, index) = self.parse_role_and_index(args)?;
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();

        let ledger = LedgerContract::new();
        let current_index = ledger
            .current_index(snapshot_ref)
            .map_err(|err| Error::native_contract(err.to_string()))?;
        if index > current_index.saturating_add(1) {
            return Err(Error::native_contract(format!(
                "Index {} exceeds current index + 1 ({})",
                index,
                current_index.saturating_add(1)
            )));
        }

        match self.find_designation_bytes(snapshot_ref, role, index)? {
            Some(bytes) => Ok(bytes),
            None => self.serialize_public_keys(&[]),
        }
    }

    fn designate_as_role(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if !engine
            .check_committee_witness()
            .map_err(|err| Error::runtime_error(err.to_string()))?
        {
            return Err(Error::invalid_operation(
                "Committee authorization required".to_string(),
            ));
        }

        if args.is_empty() {
            return Err(Error::native_contract(
                "designateAsRole requires role argument".to_string(),
            ));
        }
        let (role, _) = self.parse_role_and_index(&args[..1])?;
        if args.len() < 2 {
            return Err(Error::native_contract(
                "designateAsRole requires role and public keys arguments".to_string(),
            ));
        }

        let mut public_keys = self.parse_public_keys(&args[1])?;
        if public_keys.is_empty() || public_keys.len() > Self::MAX_NODES {
            return Err(Error::native_contract(format!(
                "Nodes count {} must be between 1 and {}",
                public_keys.len(),
                Self::MAX_NODES
            )));
        }
        public_keys.sort();

        let persisting_block = engine.persisting_block().ok_or_else(|| {
            Error::invalid_operation("Persisting block is not available".to_string())
        })?;
        let persisting_index = persisting_block.header.index;
        let designation_index = persisting_index
            .checked_add(1)
            .ok_or_else(|| Error::invalid_operation("Block index overflowed".to_string()))?;

        let context = engine.get_native_storage_context(&self.hash)?;
        let key_suffix = Self::role_key_suffix(role, designation_index);
        if engine.get_storage_item(&context, &key_suffix).is_some() {
            return Err(Error::invalid_operation(
                "Role already designated at this height".to_string(),
            ));
        }

        let serialized_keys = self.serialize_public_keys(&public_keys)?;
        engine.put_storage_item(&context, &key_suffix, &serialized_keys)?;

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let previous = match self.find_designation_bytes(
            snapshot_ref,
            role,
            designation_index.saturating_sub(1),
        )? {
            Some(bytes) => bytes,
            None => self.serialize_public_keys(&[])?,
        };

        let block_index_bytes = persisting_index.to_le_bytes().to_vec();
        let role_bytes = vec![role as u8];
        if engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            engine.emit_event(
                "Designation",
                vec![
                    role_bytes,
                    block_index_bytes,
                    previous,
                    serialized_keys.clone(),
                ],
            )?;
        } else {
            engine.emit_event("Designation", vec![role_bytes, block_index_bytes])?;
        }

        Ok(vec![1])
    }

    fn parse_role_and_index(&self, args: &[Vec<u8>]) -> Result<(Role, u32)> {
        if args.is_empty() {
            return Err(Error::native_contract("Missing role argument".to_string()));
        }
        let role = if let Some(&value) = args[0].first() {
            Role::from_u8(value).ok_or_else(|| {
                Error::native_contract(format!("Invalid role identifier: {}", value))
            })?
        } else {
            return Err(Error::native_contract("Invalid role argument".to_string()));
        };

        let index = if args.len() >= 2 {
            if args[1].len() != 4 {
                return Err(Error::native_contract(
                    "Index argument must be 4 bytes".to_string(),
                ));
            }
            let mut buffer = [0u8; 4];
            buffer.copy_from_slice(&args[1]);
            u32::from_le_bytes(buffer)
        } else {
            0
        };

        Ok((role, index))
    }

    fn find_designation_bytes(
        &self,
        snapshot: &DataCache,
        role: Role,
        index: u32,
    ) -> Result<Option<Vec<u8>>> {
        let prefix = Self::role_prefix_key(role);
        let iter = snapshot.find(Some(&prefix), SeekDirection::Backward);
        for (key, item) in iter {
            if let Some(designation_index) = Self::parse_designation_index(&key, role) {
                if designation_index <= index {
                    return Ok(Some(item.get_value()));
                }
            }
        }
        Ok(None)
    }

    fn parse_designation_index(key: &StorageKey, role: Role) -> Option<u32> {
        let suffix = key.suffix();
        if suffix.first().copied() != Some(role as u8) || suffix.len() != 5 {
            return None;
        }
        let bytes: [u8; 4] = suffix[1..].try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    }

    fn role_prefix_key(role: Role) -> StorageKey {
        StorageKey::create(Self::ID, role as u8)
    }

    fn role_key_suffix(role: Role, index: u32) -> Vec<u8> {
        let mut suffix = vec![role as u8];
        suffix.extend_from_slice(&index.to_be_bytes());
        suffix
    }

    /// Serializes public keys to bytes.
    fn serialize_public_keys(&self, public_keys: &[ECPoint]) -> Result<Vec<u8>> {
        let mut result = Vec::with_capacity(4 + public_keys.len() * 33);
        result.extend_from_slice(&(public_keys.len() as u32).to_le_bytes());
        for pubkey in public_keys {
            let encoded = pubkey
                .encode_compressed()
                .map_err(|_| Error::native_contract("Failed to encode public key".to_string()))?;
            result.extend_from_slice(&encoded);
        }
        Ok(result)
    }

    /// Parses public keys from bytes (little-endian count + compressed points).
    fn parse_public_keys(&self, data: &[u8]) -> Result<Vec<ECPoint>> {
        if data.len() < 4 {
            return Err(Error::native_contract(
                "Invalid public keys payload".to_string(),
            ));
        }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut keys = Vec::with_capacity(count);
        let mut offset = 4;
        for _ in 0..count {
            if offset + 33 > data.len() {
                return Err(Error::native_contract(
                    "Invalid public key data length".to_string(),
                ));
            }
            let mut key_bytes = [0u8; 33];
            key_bytes.copy_from_slice(&data[offset..offset + 33]);
            if key_bytes[0] != 0x02 && key_bytes[0] != 0x03 {
                return Err(Error::native_contract(
                    "Invalid public key prefix".to_string(),
                ));
            }
            let pubkey = ECPoint::decode_compressed(&key_bytes)
                .map_err(|_| Error::native_contract("Invalid public key encoding".to_string()))?;
            keys.push(pubkey);
            offset += 33;
        }
        if offset != data.len() {
            return Err(Error::native_contract(
                "Unexpected trailing data in public key payload".to_string(),
            ));
        }
        Ok(keys)
    }
}

impl NativeContract for RoleManagement {
    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "RoleManagement"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for RoleManagement {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smart_contract::StorageItem;

    fn sample_point(tag: u8) -> ECPoint {
        let mut bytes = [0u8; 33];
        bytes[0] = 0x02;
        for b in bytes.iter_mut().skip(1) {
            *b = tag;
        }
        ECPoint::decode_compressed(&bytes).expect("valid test key")
    }

    #[test]
    fn serialize_and_parse_roundtrip() {
        let contract = RoleManagement::new();
        let keys = vec![sample_point(0xAA), sample_point(0xBB)];
        let encoded = contract.serialize_public_keys(&keys).unwrap();
        let decoded = contract.parse_public_keys(&encoded).unwrap();
        assert_eq!(decoded, keys);
    }

    #[test]
    fn find_designation_returns_latest_entry() {
        let contract = RoleManagement::new();
        let cache = DataCache::new(false);
        let role = Role::Oracle;

        let key_old = StorageKey::new(RoleManagement::ID, RoleManagement::role_key_suffix(role, 5));
        let bytes_old = contract
            .serialize_public_keys(&[sample_point(0x10)])
            .unwrap();
        cache.add(key_old, StorageItem::from_bytes(bytes_old.clone()));

        let key_new = StorageKey::new(
            RoleManagement::ID,
            RoleManagement::role_key_suffix(role, 12),
        );
        let bytes_new = contract
            .serialize_public_keys(&[sample_point(0x11)])
            .unwrap();
        cache.add(key_new, StorageItem::from_bytes(bytes_new.clone()));

        let result_before = contract
            .find_designation_bytes(&cache, role, 7)
            .unwrap()
            .expect("entry");
        assert_eq!(result_before, bytes_old);

        let result_after = contract
            .find_designation_bytes(&cache, role, 99)
            .unwrap()
            .expect("entry");
        assert_eq!(result_after, bytes_new);
    }
}
