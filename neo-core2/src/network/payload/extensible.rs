use std::error::Error;
use std::fmt;

use crate::core::transaction::Witness;
use crate::crypto::hash;
use crate::io::{BinReader, BinWriter, Serializable};
use crate::util::{self, Uint160, Uint256};

const MAX_EXTENSIBLE_CATEGORY_SIZE: usize = 32;

// ConsensusCategory is a message category for consensus-related extensible
// payloads.
const CONSENSUS_CATEGORY: &str = "dBFT";

// Extensible represents a payload containing arbitrary data.
#[derive(Default)]
pub struct Extensible {
    // Category is the payload type.
    pub category: String,
    // ValidBlockStart is the starting height for a payload to be valid.
    pub valid_block_start: u32,
    // ValidBlockEnd is the height after which a payload becomes invalid.
    pub valid_block_end: u32,
    // Sender is the payload sender or signer.
    pub sender: Uint160,
    // Data is custom payload data.
    pub data: Vec<u8>,
    // Witness is payload witness.
    pub witness: Witness,

    hash: Uint256,
}

#[derive(Debug, Clone)]
struct InvalidPaddingError;

impl fmt::Display for InvalidPaddingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid padding")
    }
}

impl Error for InvalidPaddingError {}

impl Extensible {
    // NewExtensible creates a new extensible payload.
    pub fn new() -> Self {
        Extensible::default()
    }

    fn encode_binary_unsigned(&self, w: &mut BinWriter) {
        w.write_string(&self.category);
        w.write_u32_le(self.valid_block_start);
        w.write_u32_le(self.valid_block_end);
        w.write_bytes(&self.sender.0);
        w.write_var_bytes(&self.data);
    }

    // EncodeBinary implements io.Serializable.
    pub fn encode_binary(&self, w: &mut BinWriter) {
        self.encode_binary_unsigned(w);
        w.write_u8(1);
        self.witness.encode_binary(w);
    }

    fn decode_binary_unsigned(&mut self, r: &mut BinReader) {
        self.category = r.read_string(MAX_EXTENSIBLE_CATEGORY_SIZE);
        self.valid_block_start = r.read_u32_le();
        self.valid_block_end = r.read_u32_le();
        r.read_bytes(&mut self.sender.0);
        self.data = r.read_var_bytes();
    }

    // DecodeBinary implements io.Serializable.
    pub fn decode_binary(&mut self, r: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.decode_binary_unsigned(r);
        if r.read_u8() != 1 {
            if r.err().is_some() {
                return Ok(());
            }
            return Err(Box::new(InvalidPaddingError));
        }
        self.witness.decode_binary(r);
        Ok(())
    }

    // Hash returns payload hash.
    pub fn hash(&mut self) -> Uint256 {
        if self.hash == Uint256::default() {
            self.create_hash();
        }
        self.hash
    }

    // createHash creates hashes of the payload.
    fn create_hash(&mut self) {
        let mut buf = BinWriter::new();
        self.encode_binary_unsigned(&mut buf);
        self.hash = hash::sha256(buf.bytes());
    }
}
