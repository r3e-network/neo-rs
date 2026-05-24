use super::RoleManagement;
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::persistence::{DataCache, SeekDirection};
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::native::Role;
use crate::smart_contract::storage_key::StorageKey;
use crate::{ECCurve, ECPoint};
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::StackValue;
use std::convert::TryInto;

impl RoleManagement {
    pub(super) fn find_designation_bytes(
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

    pub(super) fn role_key_suffix(role: Role, index: u32) -> Vec<u8> {
        let mut suffix = vec![role as u8];
        suffix.extend_from_slice(&index.to_be_bytes());
        suffix
    }

    /// Gets designated nodes for a role at a specific block index.
    /// This is a public API used by other native contracts like Notary.
    pub fn get_designated_by_role_at(
        &self,
        snapshot: &DataCache,
        role: Role,
        index: u32,
    ) -> Result<Vec<ECPoint>> {
        match self.find_designation_bytes(snapshot, role, index)? {
            Some(bytes) => self.parse_public_keys(&bytes),
            None => Ok(vec![]),
        }
    }

    /// Serializes public keys to bytes.
    pub(crate) fn serialize_public_keys(&self, public_keys: &[ECPoint]) -> Result<Vec<u8>> {
        let value = StackValue::Array(
            public_keys
                .iter()
                .map(|pubkey| StackValue::ByteString(pubkey.as_bytes().to_vec()))
                .collect(),
        );
        BinarySerializer::serialize_stack_value(&value, &ExecutionEngineLimits::default())
            .map_err(|e| Error::native_contract(format!("Failed to serialize public keys: {e}")))
    }

    /// Parses public keys from bytes (little-endian count + compressed points).
    pub(super) fn parse_public_keys(&self, data: &[u8]) -> Result<Vec<ECPoint>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let value = BinarySerializer::deserialize_stack_value(data).map_err(|e| {
            Error::native_contract(format!("Failed to deserialize public keys: {e}"))
        })?;

        let StackValue::Array(items) = value else {
            return Err(Error::native_contract(
                "Public keys payload must be an array".to_string(),
            ));
        };

        let mut keys = Vec::with_capacity(items.len());
        for element in items {
            let bytes = element
                .to_byte_string_bytes()
                .ok_or_else(|| Error::native_contract("Invalid public key item"))?;
            let pubkey = ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &bytes)
                .map_err(|e| Error::native_contract(format!("Invalid public key encoding: {e}")))?;
            keys.push(pubkey);
        }

        Ok(keys)
    }
}
