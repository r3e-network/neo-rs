use crate::interop;

// Block represents a NEO block, it's a data structure where you can get
// block-related data from. It's similar to the Block class in the Neo .net
// framework. To use it, you need to get it via GetBlock function call.
pub struct Block {
    // Hash represents the hash (256 bit BE value in a 32 byte slice) of the
    // given block.
    pub hash: interop::Hash256,
    // Version of the block.
    pub version: i32,
    // PrevHash represents the hash (256 bit BE value in a 32 byte slice) of the
    // previous block.
    pub prev_hash: interop::Hash256,
    // MerkleRoot represents the root hash (256 bit BE value in a 32 byte slice)
    // of a transaction list.
    pub merkle_root: interop::Hash256,
    // Timestamp represents millisecond-precision block timestamp.
    pub timestamp: i32,
    // Nonce represents block nonce.
    pub nonce: i32,
    // Index represents the height of the block.
    pub index: i32,
    // PrimaryIndex represents the index of the primary node that created this block.
    pub primary_index: i32,
    // NextConsensus represents the contract address of the next miner (160 bit BE
    // value in a 20 byte slice).
    pub next_consensus: interop::Hash160,
    // TransactionsLength represents the length of block's transactions array.
    pub transactions_length: i32,
}

// BlockSR is a stateroot-enabled Neo block. It's returned from the Ledger contract's
// GetBlock method when StateRootInHeader NeoGo extension  is used. Use it only when
// you have it enabled when you need to access PrevStateRoot field, Block is sufficient
// otherwise. To get this data type ToBlockSR method of Block should be used. All of
// the fields are same as in Block except PrevStateRoot.
pub struct BlockSR {
    pub hash: interop::Hash256,
    pub version: i32,
    pub prev_hash: interop::Hash256,
    pub merkle_root: interop::Hash256,
    pub timestamp: i32,
    pub nonce: i32,
    pub index: i32,
    pub primary_index: i32,
    pub next_consensus: interop::Hash160,
    pub transactions_length: i32,
    // PrevStateRoot is a hash of the previous block's state root.
    pub prev_state_root: interop::Hash256,
}

// ToBlockSR converts Block into BlockSR for chains with StateRootInHeader option.
impl Block {
    pub fn to_block_sr(&self) -> BlockSR {
        BlockSR {
            hash: self.hash,
            version: self.version,
            prev_hash: self.prev_hash,
            merkle_root: self.merkle_root,
            timestamp: self.timestamp,
            nonce: self.nonce,
            index: self.index,
            primary_index: self.primary_index,
            next_consensus: self.next_consensus,
            transactions_length: self.transactions_length,
            prev_state_root: interop::Hash256::default(), // Assuming a default value for now
        }
    }
}
