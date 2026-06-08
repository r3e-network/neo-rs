use neo_primitives::UInt256;

/// Block data for assembly by upper layers
#[derive(Debug, Clone)]
pub struct BlockData {
    /// Block index
    pub block_index: u32,
    /// Block timestamp
    pub timestamp: u64,
    /// Block nonce
    pub nonce: u64,
    /// Primary validator index
    pub primary_index: u8,
    /// Transaction hashes included in the block
    pub transaction_hashes: Vec<UInt256>,
    /// Commit signatures from validators (`validator_index`, signature)
    pub signatures: Vec<(u8, Vec<u8>)>,
    /// Validator public keys for multi-sig witness construction
    pub validator_pubkeys: Vec<neo_crypto::ECPoint>,
    /// Required signature count (M in M-of-N multi-sig)
    pub required_signatures: usize,
}
