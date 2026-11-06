// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Protocol witness scripts, script hash caching, and wire encoding.
//! Verification and VM execution belong to higher-layer adapters.

use base64::{Engine as _, engine::general_purpose};
use neo_crypto::Crypto;
use neo_io::{Serializable, serializable::helper::get_var_size_bytes};
use neo_primitives::UInt160;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;

/// Protocol witness access without snapshot validation or VM execution.
pub trait WitnessData: neo_primitives::Verifiable {
    /// Gets the witnesses associated with this container.
    fn witnesses(&self) -> Vec<&Witness>;
    /// Gets mutable access to the witnesses associated with this container.
    fn witnesses_mut(&mut self) -> Vec<&mut Witness>;
}

// MultiSig 21/11 committee scripts fit within these existing wire limits.
const MAX_INVOCATION_SCRIPT: usize = 1024;
const MAX_VERIFICATION_SCRIPT: usize = 1024;

/// Represents a witness of a verifiable object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Witness {
    /// Arguments for the verification script.
    pub invocation_script: Vec<u8>,
    /// Verification contract script, empty for deployed contracts.
    pub verification_script: Vec<u8>,
    #[serde(skip)]
    script_hash: OnceLock<UInt160>,
}

impl Witness {
    /// Creates an empty witness.
    pub fn new() -> Self {
        Self {
            invocation_script: Vec::new(),
            verification_script: Vec::new(),
            script_hash: OnceLock::new(),
        }
    }

    /// Creates a witness with the specified scripts.
    pub fn new_with_scripts(invocation_script: Vec<u8>, verification_script: Vec<u8>) -> Self {
        Self {
            invocation_script,
            verification_script,
            script_hash: OnceLock::new(),
        }
    }

    /// Creates an empty witness.
    pub fn empty() -> Self {
        Self::new()
    }

    /// Returns the invocation script.
    pub fn invocation_script(&self) -> &[u8] {
        &self.invocation_script
    }
    /// Returns the verification script.
    pub fn verification_script(&self) -> &[u8] {
        &self.verification_script
    }

    /// Returns RIPEMD160(SHA256(verification_script)), cached as before.
    pub fn script_hash(&self) -> UInt160 {
        *self
            .script_hash
            .get_or_init(|| UInt160::from(Crypto::hash160(&self.verification_script)))
    }

    /// Gets the serialized size.
    pub fn size(&self) -> usize {
        get_var_size_bytes(&self.invocation_script) + get_var_size_bytes(&self.verification_script)
    }

    /// Converts the witness to JSON with base64-encoded scripts.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "invocation": general_purpose::STANDARD.encode(&self.invocation_script),
            "verification": general_purpose::STANDARD.encode(&self.verification_script)
        })
    }

    /// Clones scripts and any already-computed hash.
    pub fn clone_witness(&self) -> Self {
        Self {
            invocation_script: self.invocation_script.clone(),
            verification_script: self.verification_script.clone(),
            script_hash: {
                let clone_cell = OnceLock::new();
                if let Some(hash) = self.script_hash.get() {
                    let _ = clone_cell.set(*hash);
                }
                clone_cell
            },
        }
    }
}

impl Default for Witness {
    fn default() -> Self {
        Self::new()
    }
}

impl neo_primitives::Witness for Witness {
    fn invocation_script(&self) -> &[u8] {
        &self.invocation_script
    }
    fn verification_script(&self) -> &[u8] {
        &self.verification_script
    }
}

impl Serializable for Witness {
    fn size(&self) -> usize {
        self.size()
    }
    fn serialize(&self, writer: &mut neo_io::BinaryWriter) -> neo_io::IoResult<()> {
        writer.write_var_bytes(&self.invocation_script)?;
        writer.write_var_bytes(&self.verification_script)?;
        Ok(())
    }
    fn deserialize(reader: &mut neo_io::MemoryReader) -> neo_io::IoResult<Self> {
        let invocation_script = reader.read_var_bytes(MAX_INVOCATION_SCRIPT)?;
        let verification_script = reader.read_var_bytes(MAX_VERIFICATION_SCRIPT)?;
        Ok(Self {
            invocation_script,
            verification_script,
            script_hash: OnceLock::new(),
        })
    }
}

impl fmt::Display for Witness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Witness {{ invocation: {}, verification: {} }}",
            hex::encode(&self.invocation_script),
            hex::encode(&self.verification_script)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_witness_new() {
        let witness = Witness::new();
        assert!(witness.invocation_script.is_empty());
        assert!(witness.verification_script.is_empty());
        assert!(witness.script_hash.get().is_none());
    }
    #[test]
    fn test_witness_empty() {
        let witness = Witness::empty();
        assert!(witness.invocation_script.is_empty());
        assert!(witness.verification_script.is_empty());
    }
    #[test]
    fn test_witness_new_with_scripts() {
        let witness = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        assert_eq!(witness.invocation_script, vec![1, 2, 3]);
        assert_eq!(witness.verification_script, vec![4, 5, 6]);
    }
    #[test]
    fn test_witness_size() {
        assert_eq!(
            Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]).size(),
            8
        );
    }
    #[test]
    fn witness_size_matches_serialized_length_at_var_size_boundaries() {
        for len in [0, 1, 252, 253, 254, 1024] {
            let witness = Witness::new_with_scripts(vec![0xAA; len], vec![0xBB; len]);
            let mut writer = neo_io::BinaryWriter::new();
            Serializable::serialize(&witness, &mut writer).unwrap();
            assert_eq!(witness.size(), writer.as_bytes().len());
            assert_eq!(
                witness.size(),
                get_var_size_bytes(&witness.invocation_script)
                    + get_var_size_bytes(&witness.verification_script)
            );
        }
    }
    #[test]
    fn test_witness_clone() {
        let original = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        let cloned = original.clone_witness();
        assert_eq!(original.invocation_script, cloned.invocation_script);
        assert_eq!(original.verification_script, cloned.verification_script);
    }
    #[test]
    fn test_witness_serialization() {
        let witness = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        let mut writer = neo_io::BinaryWriter::new();
        Serializable::serialize(&witness, &mut writer).unwrap();
        let bytes = writer.to_bytes();
        let mut reader = neo_io::MemoryReader::new(&bytes);
        let decoded = <Witness as Serializable>::deserialize(&mut reader).unwrap();
        assert_eq!(witness.invocation_script, decoded.invocation_script);
        assert_eq!(witness.verification_script, decoded.verification_script);
    }
    #[test]
    fn historical_public_script_mutation_does_not_reset_cached_hash() {
        let mut witness = Witness::new_with_scripts(vec![], vec![1]);
        let original_hash = witness.script_hash();
        witness.verification_script = vec![2];
        assert_eq!(witness.script_hash(), original_hash);
        assert_eq!(witness.clone_witness().script_hash(), original_hash);
    }
}
