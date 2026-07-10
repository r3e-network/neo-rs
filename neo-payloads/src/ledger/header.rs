//! Block header.

use crate::witness::Witness;
use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};

/// Represents the header of a block.
#[derive(Debug, Serialize, Deserialize)]
pub struct Header {
    version: u32,
    prev_hash: UInt256,
    merkle_root: UInt256,
    timestamp: u64,
    nonce: u64,
    index: u32,
    primary_index: u8,
    next_consensus: UInt160,

    /// The witness of the block.
    pub witness: Witness,

    // Interior-mutable lazy hash cache (so `hash()` is `&self`, matching the
    // former ledger `BlockHeader`). Excluded from (de)serialization and Clone.
    #[serde(skip)]
    _hash: parking_lot::Mutex<Option<UInt256>>,
}

impl Clone for Header {
    fn clone(&self) -> Self {
        Self {
            version: self.version,
            prev_hash: self.prev_hash,
            merkle_root: self.merkle_root,
            timestamp: self.timestamp,
            nonce: self.nonce,
            index: self.index,
            primary_index: self.primary_index,
            next_consensus: self.next_consensus,
            witness: self.witness.clone(),
            _hash: parking_lot::Mutex::new(*self._hash.lock()),
        }
    }
}

impl Header {
    /// Creates a new header.
    pub fn new() -> Self {
        Self {
            version: 0,
            prev_hash: UInt256::default(),
            merkle_root: UInt256::default(),
            timestamp: 0,
            nonce: 0,
            index: 0,
            primary_index: 0,
            next_consensus: UInt160::default(),
            witness: Witness::new(),
            _hash: parking_lot::Mutex::new(None),
        }
    }

    /// Back-compat constructor taking a witness vector (Neo N3 headers carry
    /// exactly one witness; the first is used). Replaces the former
    /// `ledger::BlockHeader::new_with_witnesses(.., witnesses: Vec<Witness>)`.
    // Rationale: header constructors expose the full serialized protocol field
    // list explicitly, matching the reference node's header shape.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_witnesses(
        version: u32,
        prev_hash: UInt256,
        merkle_root: UInt256,
        timestamp: u64,
        nonce: u64,
        index: u32,
        primary_index: u8,
        next_consensus: UInt160,
        witnesses: Vec<Witness>,
    ) -> Self {
        Self::from_parts(
            version,
            prev_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witnesses.into_iter().next().unwrap_or_default(),
        )
    }

    /// Creates a header from all of its parts (Neo N3 headers carry exactly one
    /// witness). Replaces the former `ledger::BlockHeader::new_with_witnesses(.., Vec<Witness>)`.
    // Rationale: header construction is a protocol field list; grouping these
    // values would hide the exact serialized hash order.
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        version: u32,
        prev_hash: UInt256,
        merkle_root: UInt256,
        timestamp: u64,
        nonce: u64,
        index: u32,
        primary_index: u8,
        next_consensus: UInt160,
        witness: Witness,
    ) -> Self {
        Self {
            version,
            prev_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witness,
            _hash: parking_lot::Mutex::new(None),
        }
    }

    /// Gets the version of the block.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Sets the version of the block.
    pub fn set_version(&mut self, value: u32) {
        self.version = value;
        *self._hash.lock() = None;
    }

    /// Gets the hash of the previous block.
    pub fn prev_hash(&self) -> &UInt256 {
        &self.prev_hash
    }

    /// Sets the hash of the previous block.
    pub fn set_prev_hash(&mut self, value: UInt256) {
        self.prev_hash = value;
        *self._hash.lock() = None;
    }

    /// Gets the merkle root of the transactions.
    pub fn merkle_root(&self) -> &UInt256 {
        &self.merkle_root
    }

    /// Sets the merkle root of the transactions.
    pub fn set_merkle_root(&mut self, value: UInt256) {
        self.merkle_root = value;
        *self._hash.lock() = None;
    }

    /// Gets the timestamp of the block.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Sets the timestamp of the block.
    pub fn set_timestamp(&mut self, value: u64) {
        self.timestamp = value;
        *self._hash.lock() = None;
    }

    /// Gets the nonce of the block.
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Sets the nonce of the block.
    pub fn set_nonce(&mut self, value: u64) {
        self.nonce = value;
        *self._hash.lock() = None;
    }

    /// Gets the index of the block.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Sets the index of the block.
    pub fn set_index(&mut self, value: u32) {
        self.index = value;
        *self._hash.lock() = None;
    }

    /// Gets the primary index of the consensus node.
    pub fn primary_index(&self) -> u8 {
        self.primary_index
    }

    /// Sets the primary index of the consensus node.
    pub fn set_primary_index(&mut self, value: u8) {
        self.primary_index = value;
        *self._hash.lock() = None;
    }

    /// Gets the next consensus address.
    pub fn next_consensus(&self) -> &UInt160 {
        &self.next_consensus
    }

    /// Sets the next consensus address.
    pub fn set_next_consensus(&mut self, value: UInt160) {
        self.next_consensus = value;
        *self._hash.lock() = None;
    }

    /// Gets the hash of the header.
    pub fn hash(&self) -> UInt256 {
        match self.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                tracing::error!(
                    target: "neo::payloads::header",
                    error = %error,
                    "Failed to hash header"
                );
                UInt256::zero()
            }
        }
    }

    /// Serializes this header to its canonical Neo RPC JSON object, matching C#
    /// `Header.ToJson` (field set and ordering: hash, size, version,
    /// previousblockhash, merkleroot, time, nonce as `{:016X}`, index, primary,
    /// nextconsensus, witnesses). This is the single source of truth for the
    /// header wire-JSON shared by the RPC server and client; callers that serve
    /// `getblock`/`getblockheader` add the contextual `confirmations` and
    /// optional `nextblockhash` fields on top.
    pub fn to_json(
        &self,
        settings: &ProtocolSettings,
    ) -> serde_json::Map<String, serde_json::Value> {
        use serde_json::{Value, json};
        let hash = self.hash();
        let mut json = serde_json::Map::new();
        json.insert("hash".to_string(), Value::String(hash.to_string()));
        json.insert("size".to_string(), json!(self.size()));
        json.insert("version".to_string(), json!(self.version()));
        json.insert(
            "previousblockhash".to_string(),
            Value::String(self.prev_hash().to_string()),
        );
        json.insert(
            "merkleroot".to_string(),
            Value::String(self.merkle_root().to_string()),
        );
        json.insert("time".to_string(), json!(self.timestamp()));
        json.insert(
            "nonce".to_string(),
            Value::String(format!("{:016X}", self.nonce())),
        );
        json.insert("index".to_string(), json!(self.index()));
        json.insert("primary".to_string(), json!(self.primary_index()));
        json.insert(
            "nextconsensus".to_string(),
            Value::String(
                self.next_consensus()
                    .to_address_with_version(settings.address_version),
            ),
        );
        json.insert(
            "witnesses".to_string(),
            Value::Array(vec![self.witness.to_json()]),
        );
        json
    }

    /// Gets the hash of the header, failing closed if unsigned serialization
    /// fails.
    pub fn try_hash(&self) -> CoreResult<UInt256> {
        if let Some(hash) = *self._hash.lock() {
            return Ok(hash);
        }

        let hash_data = self.try_get_hash_data()?;
        // Neo N3 block hashes use single SHA-256 over the unsigned header payload.
        let hash = UInt256::from(neo_crypto::Crypto::sha256(&hash_data));
        *self._hash.lock() = Some(hash);
        Ok(hash)
    }
}

// Use macro to reduce boilerplate
neo_io::impl_default_via_new!(Header);

impl crate::VerifiableExt for Header {
    fn witnesses(&self) -> Vec<&crate::Witness> {
        vec![&self.witness]
    }

    fn witnesses_mut(&mut self) -> Vec<&mut crate::Witness> {
        vec![&mut self.witness]
    }

    fn to_verifiable_container(&self) -> Option<std::sync::Arc<crate::VerifiableContainer>> {
        Some(std::sync::Arc::new(crate::VerifiableContainer::from(
            self.clone(),
        )))
    }
}

impl neo_primitives::SerializablePayload for Header {
    fn hash_data(&self) -> Vec<u8> {
        Header::hash_data(self)
    }

    fn hash(&self) -> UInt256 {
        self.try_hash().unwrap_or_default()
    }

    fn witness_count(&self) -> usize {
        1
    }

    fn invocation_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            self.witness.invocation_script.as_slice()
        } else {
            &[]
        }
    }

    fn verification_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            self.witness.verification_script.as_slice()
        } else {
            &[]
        }
    }
}

// ============================================================================
// Serialization impls (inlined from header/serialization.rs)
// ============================================================================

impl Header {
    /// Returns the unsigned serialization used for hashing.
    pub fn hash_data(&self) -> Vec<u8> {
        match self.try_get_hash_data() {
            Ok(data) => data,
            Err(err) => {
                tracing::error!("Failed to serialize header unsigned data: {err}");
                Vec::new()
            }
        }
    }

    /// Returns the unsigned serialization used for hashing.
    pub fn try_get_hash_data(&self) -> CoreResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)?;
        Ok(writer.into_bytes())
    }

    /// Serialize without witness.
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.version)?;
        writer.write_serializable(&self.prev_hash)?;
        writer.write_serializable(&self.merkle_root)?;
        writer.write_u64(self.timestamp)?;
        writer.write_u64(self.nonce)?;
        writer.write_u32(self.index)?;
        writer.write_u8(self.primary_index)?;
        writer.write_serializable(&self.next_consensus)?;
        Ok(())
    }
}

impl neo_primitives::Verifiable for Header {
    fn hash(&self) -> neo_primitives::error::PrimitiveResult<neo_primitives::UInt256> {
        let data = self.try_get_hash_data().map_err(|e| {
            neo_primitives::error::PrimitiveError::invalid_data(format!(
                "header serialization failed: {e}"
            ))
        })?;
        Ok(neo_primitives::UInt256::from(neo_crypto::Crypto::sha256(
            &data,
        )))
    }
    fn hash_data(&self) -> Vec<u8> {
        let mut writer = neo_io::BinaryWriter::new();
        if self.serialize_unsigned(&mut writer).is_err() {
            return Vec::new();
        }
        writer.into_bytes()
    }

    fn verify(&self) -> bool {
        true
    }
}

impl Serializable for Header {
    fn size(&self) -> usize {
        // C# Header.Size: the witness is serialized as a 1-element var-array,
        // so the var-int count byte is part of the wire size (Header.cs:90-99).
        4 + 32 + 32 + 8 + 8 + 4 + 1 + 20 + 1 + <Witness as Serializable>::size(&self.witness)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.version)?;
        <UInt256 as Serializable>::serialize(&self.prev_hash, writer)?;
        <UInt256 as Serializable>::serialize(&self.merkle_root, writer)?;
        writer.write_u64(self.timestamp)?;
        writer.write_u64(self.nonce)?;
        writer.write_u32(self.index)?;
        writer.write_u8(self.primary_index)?;
        <UInt160 as Serializable>::serialize(&self.next_consensus, writer)?;
        // C# Header.Serialize writes `new Witness[] { Witness }` — a var-array
        // with its count byte (Header.cs:163-167).
        writer.write_var_int(1)?;
        <Witness as Serializable>::serialize(&self.witness, writer)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let version = reader.read_u32()?;
        if version > 0 {
            return Err(neo_io::IoError::invalid_data("Header version must be 0"));
        }
        let prev_hash = <UInt256 as Serializable>::deserialize(reader)?;
        let merkle_root = <UInt256 as Serializable>::deserialize(reader)?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let index = reader.read_u32()?;
        let primary_index = reader.read_u8()?;
        let next_consensus = <UInt160 as Serializable>::deserialize(reader)?;
        // C# Header.Deserialize reads a witness array capped at 1 and requires
        // exactly one entry (Header.cs:116-122).
        let witness_count = reader.read_var_int(1)?;
        if witness_count != 1 {
            return Err(neo_io::IoError::InvalidData {
                context: "Header.witness".to_string(),
                value: format!("expected 1 witness in Header, got {witness_count}"),
            });
        }
        let witness = <Witness as Serializable>::deserialize(reader)?;
        Ok(Self {
            version,
            prev_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witness,
            _hash: parking_lot::Mutex::new(None),
        })
    }
}

#[cfg(test)]
#[path = "../tests/ledger/header.rs"]
mod tests;
