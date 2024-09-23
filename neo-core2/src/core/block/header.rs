use serde::{Deserialize, Serialize};
use serde_json::{self, json};
use std::fmt;
use std::str::FromStr;

use crate::core::transaction::Witness;
use crate::crypto::hash;
use crate::encoding::address;
use crate::io::{BinReader, BinWriter};
use crate::util::{Uint160, Uint256};

// VersionInitial is the default Neo block version.
pub const VERSION_INITIAL: u32 = 0;

// Header holds the base info of a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    // Version of the block.
    pub version: u32,

    // hash of the previous block.
    pub prev_hash: Uint256,

    // Root hash of a transaction list.
    pub merkle_root: Uint256,

    // Timestamp is a millisecond-precision timestamp.
    // The time stamp of each block must be later than the previous block's time stamp.
    // Generally, the difference between two block's time stamps is about 15 seconds and imprecision is allowed.
    // The height of the block must be exactly equal to the height of the previous block plus 1.
    pub timestamp: u64,

    // Nonce is block random number.
    pub nonce: u64,

    // index/height of the block
    pub index: u32,

    // Contract address of the next miner
    pub next_consensus: Uint160,

    // Script used to validate the block
    pub script: Witness,

    // StateRootEnabled specifies if the header contains state root.
    pub state_root_enabled: bool,
    // PrevStateRoot is the state root of the previous block.
    pub prev_state_root: Option<Uint256>,
    // PrimaryIndex is the index of the primary consensus node for this block.
    pub primary_index: u8,

    // Hash of this block, created when binary encoded (double SHA256).
    pub hash: Option<Uint256>,
}

// baseAux is used to marshal/unmarshal to/from JSON, it's almost the same
// as original Base, but with Nonce and NextConsensus fields differing and
// Hash added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BaseAux {
    hash: Uint256,
    version: u32,
    previousblockhash: Uint256,
    merkleroot: Uint256,
    time: u64,
    nonce: String,
    index: u32,
    nextconsensus: String,
    primary: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    previousstateroot: Option<Uint256>,
    witnesses: Vec<Witness>,
}

impl Header {
    // Hash returns the hash of the block.
    pub fn hash(&mut self) -> Uint256 {
        if self.hash.is_none() {
            self.create_hash();
        }
        self.hash.unwrap()
    }

    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) {
        self.decode_hashable_fields(br);
        let witness_count = br.read_var_uint();
        if br.err.is_none() && witness_count != 1 {
            br.err = Some("wrong witness count".to_string());
            return;
        }

        self.script.decode_binary(br);
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) {
        self.encode_hashable_fields(bw);
        bw.write_var_uint(1);
        self.script.encode_binary(bw);
    }

    // createHash creates the hash of the block.
    // When calculating the hash value of the block, instead of processing the entire block,
    // only the header (without the signatures) is added as an input for the hash. It differs
    // from the complete block only in that it doesn't contain transactions, but their hashes
    // are used for MerkleRoot hash calculation. Therefore, adding/removing/changing any
    // transaction affects the header hash and there is no need to use the complete block for
    // hash calculation.
    fn create_hash(&mut self) {
        let mut buf = Vec::new();
        let mut bw = BinWriter::new(&mut buf);
        // No error can occur while encoding hashable fields.
        self.encode_hashable_fields(&mut bw);

        self.hash = Some(hash::sha256(&buf));
    }

    // encodeHashableFields will only encode the fields used for hashing.
    // see Hash() for more information about the fields.
    fn encode_hashable_fields(&self, bw: &mut BinWriter) {
        bw.write_u32_le(self.version);
        bw.write_bytes(&self.prev_hash.0);
        bw.write_bytes(&self.merkle_root.0);
        bw.write_u64_le(self.timestamp);
        bw.write_u64_le(self.nonce);
        bw.write_u32_le(self.index);
        bw.write_u8(self.primary_index);
        bw.write_bytes(&self.next_consensus.0);
        if self.state_root_enabled {
            if let Some(prev_state_root) = &self.prev_state_root {
                bw.write_bytes(&prev_state_root.0);
            }
        }
    }

    // decodeHashableFields decodes the fields used for hashing.
    // see Hash() for more information about the fields.
    fn decode_hashable_fields(&mut self, br: &mut BinReader) {
        self.version = br.read_u32_le();
        br.read_bytes(&mut self.prev_hash.0);
        br.read_bytes(&mut self.merkle_root.0);
        self.timestamp = br.read_u64_le();
        self.nonce = br.read_u64_le();
        self.index = br.read_u32_le();
        self.primary_index = br.read_u8();
        br.read_bytes(&mut self.next_consensus.0);
        if self.state_root_enabled {
            let mut prev_state_root = [0u8; 32];
            br.read_bytes(&mut prev_state_root);
            self.prev_state_root = Some(Uint256(prev_state_root));
        }

        // Make the hash of the block here so we dont need to do this
        // again.
        if br.err.is_none() {
            self.create_hash();
        }
    }

    // MarshalJSON implements the json.Marshaler interface.
    pub fn marshal_json(&self) -> Result<String, serde_json::Error> {
        let aux = BaseAux {
            hash: self.hash.unwrap_or_default(),
            version: self.version,
            previousblockhash: self.prev_hash,
            merkleroot: self.merkle_root,
            time: self.timestamp,
            nonce: format!("{:016X}", self.nonce),
            index: self.index,
            primary: self.primary_index,
            nextconsensus: address::uint160_to_string(&self.next_consensus),
            witnesses: vec![self.script.clone()],
            previousstateroot: if self.state_root_enabled {
                self.prev_state_root.clone()
            } else {
                None
            },
        };
        serde_json::to_string(&aux)
    }

    // UnmarshalJSON implements the json.Unmarshaler interface.
    pub fn unmarshal_json(&mut self, data: &str) -> Result<(), Box<dyn std::error::Error>> {
        let aux: BaseAux = serde_json::from_str(data)?;
        let next_consensus = address::string_to_uint160(&aux.nextconsensus)?;

        let nonce = if !aux.nonce.is_empty() {
            u64::from_str_radix(&aux.nonce, 16)?
        } else {
            0
        };

        if aux.witnesses.len() != 1 {
            return Err("wrong number of witnesses".into());
        }

        self.version = aux.version;
        self.prev_hash = aux.previousblockhash;
        self.merkle_root = aux.merkleroot;
        self.timestamp = aux.time;
        self.nonce = nonce;
        self.index = aux.index;
        self.primary_index = aux.primary;
        self.next_consensus = next_consensus;
        self.script = aux.witnesses[0].clone();
        if self.state_root_enabled {
            if aux.previousstateroot.is_none() {
                return Err("'previousstateroot' is empty".into());
            }
            self.prev_state_root = aux.previousstateroot;
        }
        if aux.hash != self.hash() {
            return Err("json 'hash' doesn't match block hash".into());
        }
        Ok(())
    }
}
