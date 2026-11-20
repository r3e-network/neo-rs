//! State Root Implementation
//!
//! Restored from neo-state-service after consolidation.
//! Matches C# Neo.Plugins.StateService.Network.StateRoot.

use neo_crypto::Crypto;
use neo_io_crate::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_native_contracts::native::{Role, role_management::RoleManagement};
use neo_primitives::error::PrimitiveResult;
use neo_primitives::{UInt256, Verifiable};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::Witness;
use neo_smart_contract::contract::Contract;
use neo_smart_contract::helper::Helper;
use neo_storage::persistence::DataCache;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// State service category for extensible payloads.
/// Matches C# Neo.Plugins.StateService category string.
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

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
    cached_hash: Option<CachedHash>,
}

/// Digest of the signed payload together with the exact signed field values it
/// was derived from.
///
/// `version`, `index` and `root_hash` are public and mutable, so a bare
/// `Option<UInt256>` cache would keep returning the digest of a payload that no
/// longer exists after a mutation — making `hash()` and `get_unsigned_data()`
/// disagree. Storing the inputs lets `hash()` detect an out-of-date entry.
#[derive(Debug, Clone)]
struct CachedHash {
    version: u8,
    index: u32,
    root_hash: UInt256,
    digest: UInt256,
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
        if let Some(cached) = &self.cached_hash
            && cached.version == self.version
            && cached.index == self.index
            && cached.root_hash == self.root_hash
        {
            return cached.digest;
        }

        let unsigned_data = self.get_unsigned_data();
        let digest = UInt256::from(Crypto::hash256(&unsigned_data));
        self.cached_hash = Some(CachedHash {
            version: self.version,
            index: self.index,
            root_hash: self.root_hash,
            digest,
        });
        digest
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

    /// Verifies the witness attached to this state root against the designated state validators.
    /// Mirrors C# `StateRoot.Verify` logic and enforces:
    /// - witness is present
    /// - verification script matches the expected BFT multi-sig contract
    /// - signatures satisfy the required threshold and are valid over `network || hash`
    pub fn verify(&self, settings: &ProtocolSettings, snapshot: &DataCache) -> bool {
        let Some(witness) = &self.witness else {
            return false;
        };

        // Resolve the validator set at the given index (Role.StateValidator).
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
        if witness.verification_script() != expected_script.as_slice() {
            debug!(
                target: "neo",
                index = self.index,
                "state root verification script mismatch"
            );
            return false;
        }

        let Some(signatures) =
            Helper::parse_multi_sig_invocation(witness.invocation_script(), required_signatures)
        else {
            return false;
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
        let mut hash_clone = self.clone();
        let hash = StateRoot::hash(&mut hash_clone);
        let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
        sign_data.extend_from_slice(&settings.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.to_array());

        // Verify each signature against the corresponding public key.
        for (sig, key_bytes) in signatures.iter().zip(encoded_keys.iter()) {
            if !Crypto::verify_signature(&sign_data, sig, key_bytes) {
                return false;
            }
        }

        true
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
                "Expected 0 or 1 witness, got {witness_count}"
            )));
        };

        Ok(root)
    }
}

impl Verifiable for StateRoot {
    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> PrimitiveResult<UInt256> {
        let mut clone = self.clone();
        Ok(StateRoot::hash(&mut clone))
    }

    fn hash_data(&self) -> Vec<u8> {
        self.get_unsigned_data()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_one() -> UInt256 {
        UInt256::from([0x11u8; 32])
    }

    fn root_two() -> UInt256 {
        UInt256::from([0x22u8; 32])
    }

    #[test]
    fn repeated_hash_returns_cached_digest() {
        let mut root = StateRoot::new_current(7, root_one());
        let first = StateRoot::hash(&mut root);
        assert_eq!(first, StateRoot::hash(&mut root));
    }

    #[test]
    fn mutating_index_invalidates_the_cached_hash() {
        let mut root = StateRoot::new_current(7, root_one());
        let original = StateRoot::hash(&mut root);

        root.index = 8;

        let recomputed = StateRoot::hash(&mut root);
        assert_eq!(
            recomputed,
            UInt256::from(Crypto::hash256(&root.get_unsigned_data())),
            "hash() must follow the current unsigned payload"
        );
        assert_ne!(recomputed, original);
    }

    #[test]
    fn mutating_root_hash_invalidates_the_cached_hash() {
        let mut root = StateRoot::new_current(7, root_one());
        let original = StateRoot::hash(&mut root);

        root.root_hash = root_two();

        let recomputed = StateRoot::hash(&mut root);
        assert_eq!(
            recomputed,
            UInt256::from(Crypto::hash256(&root.get_unsigned_data()))
        );
        assert_ne!(recomputed, original);
    }

    #[test]
    fn mutating_version_invalidates_the_cached_hash() {
        let mut root = StateRoot::new_current(7, root_one());
        let original = StateRoot::hash(&mut root);

        root.version = CURRENT_VERSION + 1;

        let recomputed = StateRoot::hash(&mut root);
        assert_eq!(
            recomputed,
            UInt256::from(Crypto::hash256(&root.get_unsigned_data()))
        );
        assert_ne!(recomputed, original);
    }

    #[test]
    fn restoring_signed_fields_restores_the_cached_digest() {
        let mut root = StateRoot::new_current(7, root_one());
        let original = StateRoot::hash(&mut root);

        root.index = 99;
        let _ = StateRoot::hash(&mut root);
        root.index = 7;

        assert_eq!(StateRoot::hash(&mut root), original);
    }
}
