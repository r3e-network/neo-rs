//! State Root Implementation
//!
//! Matches C# Neo.Plugins.StateService.Network.StateRoot exactly.

use crate::cryptography::Crypto;
use crate::cryptography::NeoHash;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::network::p2p::payloads::Witness;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::{
    helper::Helper,
    native::{role_management::RoleManagement, Role},
    Contract,
};
use crate::UInt256;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Current version of state root format.
pub const CURRENT_VERSION: u8 = 0x00;

/// Represents a state root for block state verification.
/// This matches the C# StateRoot class exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRoot {
    /// Version of the state root format.
    pub version: u8,
    /// Block index this state root corresponds to.
    pub index: u32,
    /// Root hash of the state Merkle trie.
    pub root_hash: UInt256,
    /// Witness for verification (optional until validated).
    #[serde(skip)]
    pub witness: Option<Witness>,
    /// Cached hash of the state root.
    #[serde(skip)]
    cached_hash: Option<UInt256>,
}

impl StateRoot {
    /// Creates a new state root.
    pub fn new(version: u8, index: u32, root_hash: UInt256) -> Self {
        Self {
            version,
            index,
            root_hash,
            witness: None,
            cached_hash: None,
        }
    }

    /// Creates a new state root with current version.
    pub fn new_current(index: u32, root_hash: UInt256) -> Self {
        Self::new(CURRENT_VERSION, index, root_hash)
    }

    /// Gets the hash of this state root (excluding witness).
    pub fn hash(&mut self) -> UInt256 {
        if let Some(hash) = &self.cached_hash {
            return *hash;
        }

        let unsigned_data = self.get_unsigned_data();
        // Matches C# `IVerifiable.CalculateHash`: single SHA-256 of unsigned serialization.
        let hash = UInt256::from(NeoHash::sha256(&unsigned_data));
        self.cached_hash = Some(hash);
        hash
    }

    /// Gets the unsigned serialized data (for hashing).
    pub fn get_unsigned_data(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if let Err(err) = self.serialize_unsigned(&mut writer) {
            warn!("StateRoot unsigned serialization failed: {err}");
            return Vec::new();
        }
        writer.into_bytes()
    }

    /// Serializes the unsigned portion of the state root.
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_byte(self.version)?;
        writer.write_u32(self.index)?;
        writer.write_serializable(&self.root_hash)?;
        Ok(())
    }

    /// Deserializes the unsigned portion of the state root.
    pub fn deserialize_unsigned(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_byte()?;
        let index = reader.read_u32()?;
        let root_hash = <UInt256 as Serializable>::deserialize(reader)?;

        Ok(Self {
            version,
            index,
            root_hash,
            witness: None,
            cached_hash: None,
        })
    }

    /// Converts to JSON representation.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "version": self.version,
            "index": self.index,
            "roothash": self.root_hash.to_string(),
            "witnesses": match &self.witness {
                Some(w) => serde_json::json!([{
                    "invocation": hex::encode(&w.invocation_script),
                    "verification": hex::encode(&w.verification_script)
                }]),
                None => serde_json::json!([]),
            }
        })
    }

    /// Verifies the witness attached to this state root against the designated state validators.
    /// Mirrors C# `StateRoot.Verify` logic and enforces:
    /// - witness is present
    /// - verification script matches the expected BFT multi-sig contract
    /// - signatures satisfy the required threshold and are valid over `network || hash`
    pub fn verify(&self, settings: &ProtocolSettings, snapshot: &DataCache) -> bool {
        let witness = match &self.witness {
            Some(witness) => witness,
            None => return false,
        };

        // Resolve the validator set at the given index (Role.StateValidator).
        //
        // Parity with Neo.Plugins.StateService: if no state validators are designated, state root
        // verification must fail.
        let mut validators = match RoleManagement::new().get_designated_by_role_at(
            snapshot,
            Role::StateValidator,
            self.index,
        ) {
            Ok(list) if !list.is_empty() => list,
            Ok(_) => {
                debug!(
                    target: "neo",
                    index = self.index,
                    "state root verification aborted: no designated state validators"
                );
                return false;
            }
            Err(error) => {
                debug!(
                    target: "neo",
                    %error,
                    index = self.index,
                    "state root verification aborted: failed to load designated state validators"
                );
                return false;
            }
        };

        // BFT threshold: n - (n-1)/3
        let required_signatures = validators.len() - (validators.len().saturating_sub(1)) / 3;
        if required_signatures == 0 {
            warn!(target: "neo", index = self.index, "state root verification aborted: invalid quorum");
            return false;
        }

        // Build the canonical multi-sig script (keys are sorted internally).
        let expected_script =
            Contract::create_multi_sig_redeem_script(required_signatures, &validators);
        if witness.verification_script != expected_script {
            debug!(
                target: "neo",
                index = self.index,
                "state root verification script mismatch"
            );
            return false;
        }

        let signatures = match Helper::parse_multi_sig_invocation(
            &witness.invocation_script,
            required_signatures,
        ) {
            Some(sigs) => sigs,
            None => return false,
        };

        // Sort validators to match the redeem script order.
        validators.sort();
        let mut encoded_keys = Vec::with_capacity(validators.len());
        for key in validators {
            match key.encode_point(true) {
                Ok(bytes) => encoded_keys.push(bytes),
                Err(_) => return false,
            }
        }

        // Sign data: network magic (LE) + state root hash
        let mut sign_data = [0u8; 4 + UInt256::LENGTH];
        sign_data[..4].copy_from_slice(&settings.network.to_le_bytes());
        let mut hashable = self.clone();
        sign_data[4..].copy_from_slice(&hashable.hash().to_array());

        // Verify multi-signature set using Neo's CheckMultisig semantics:
        // signatures must be in the same order as the corresponding public keys, but
        // public keys may be skipped (i.e. signatures are a subsequence of keys).
        let mut sig_index = 0usize;
        let mut key_index = 0usize;
        while sig_index < required_signatures && key_index < encoded_keys.len() {
            if Crypto::verify_signature_bytes(
                &sign_data,
                &signatures[sig_index],
                &encoded_keys[key_index],
            ) {
                sig_index += 1;
            }
            key_index += 1;
        }

        sig_index == required_signatures
    }
}

impl Serializable for StateRoot {
    fn size(&self) -> usize {
        1 + // version
        4 + // index
        32 + // root_hash
        match &self.witness {
            Some(w) => 1 + w.size(), // array prefix + witness
            None => 1, // empty array
        }
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        match &self.witness {
            Some(w) => {
                writer.write_var_uint(1)?;
                <Witness as Serializable>::serialize(w, writer)?;
            }
            None => {
                writer.write_var_uint(0)?;
            }
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let mut root = Self::deserialize_unsigned(reader)?;

        let witness_count = reader.read_var_int(1)?;
        root.witness = if witness_count == 0 {
            None
        } else if witness_count == 1 {
            Some(<Witness as Serializable>::deserialize(reader)?)
        } else {
            return Err(IoError::invalid_data(format!(
                "Expected 0 or 1 witness, got {}",
                witness_count
            )));
        };

        Ok(root)
    }
}

impl Default for StateRoot {
    fn default() -> Self {
        Self::new(CURRENT_VERSION, 0, UInt256::zero())
    }
}

#[cfg(test)]
mod multisig_verify_tests {
    use super::*;
    use crate::persistence::data_cache::DataCache;
    use crate::smart_contract::native::{role_management::RoleManagement, NativeContract, Role};
    use crate::smart_contract::Contract;
    use crate::smart_contract::{StorageItem, StorageKey};
    use crate::wallets::KeyPair;
    use neo_vm::op_code::OpCode;

    #[test]
    fn verify_accepts_signatures_as_subsequence_of_sorted_keys() {
        let mut settings = ProtocolSettings::default_settings();
        settings.network = 0x0A0B0C0D;

        // Build 4 standby validators; required signatures for BFT quorum is 3.
        let mut pairs = (0..4)
            .map(|_| {
                let keypair = KeyPair::generate().expect("generate keypair");
                let pubkey = keypair
                    .get_public_key_point()
                    .expect("public key point from keypair");
                (pubkey, keypair)
            })
            .collect::<Vec<_>>();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let validators = pairs.iter().map(|(pk, _)| pk.clone()).collect::<Vec<_>>();
        settings.standby_committee = validators.clone();
        settings.validators_count = validators.len() as i32;

        let required = validators.len() - (validators.len().saturating_sub(1)) / 3;
        assert_eq!(required, 3);

        let mut root = StateRoot::new_current(123, UInt256::from_bytes(&[7u8; 32]).unwrap());
        let hash = root.hash();

        let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
        sign_data.extend_from_slice(&settings.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.to_array());

        // Sign with the last 3 keys (skip the first key in the sorted order).
        let mut invocation = Vec::new();
        for (_pubkey, keypair) in pairs.iter().skip(1).take(required) {
            let signature = keypair.sign(&sign_data).expect("sign state root");
            invocation.push(OpCode::PUSHDATA1 as u8);
            invocation.push(signature.len() as u8);
            invocation.extend_from_slice(&signature);
        }

        let verification_script = Contract::create_multi_sig_redeem_script(required, &validators);
        root.witness = Some(Witness::new_with_scripts(invocation, verification_script));

        // Seed a RoleManagement designation so verification uses state validators.
        let snapshot = DataCache::new(false);
        let mut suffix = vec![Role::StateValidator as u8];
        suffix.extend_from_slice(&root.index.to_be_bytes());
        let key = StorageKey::new(RoleManagement::new().id(), suffix);
        let mut value = Vec::with_capacity(4 + 33 * validators.len());
        value.extend_from_slice(&(validators.len() as u32).to_le_bytes());
        for validator in &validators {
            let encoded = validator
                .encode_compressed()
                .expect("validator key must encode");
            value.extend_from_slice(&encoded);
        }
        snapshot.add(key, StorageItem::from_bytes(value));
        assert!(root.verify(&settings, &snapshot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_root_creation() {
        let root_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let state_root = StateRoot::new_current(100, root_hash);

        assert_eq!(state_root.version, CURRENT_VERSION);
        assert_eq!(state_root.index, 100);
        assert_eq!(state_root.root_hash, root_hash);
        assert!(state_root.witness.is_none());
    }

    #[test]
    fn test_state_root_serialization() {
        use crate::neo_io::Serializable;
        let root_hash = UInt256::from_bytes(&[2u8; 32]).unwrap();
        let state_root = StateRoot::new_current(12345, root_hash);

        let mut writer = BinaryWriter::new();
        Serializable::serialize(&state_root, &mut writer).unwrap();
        let data = writer.into_bytes();

        let mut reader = MemoryReader::new(&data);
        let deserialized: StateRoot = Serializable::deserialize(&mut reader).unwrap();

        assert_eq!(deserialized.version, state_root.version);
        assert_eq!(deserialized.index, state_root.index);
        assert_eq!(deserialized.root_hash, state_root.root_hash);
    }
}
