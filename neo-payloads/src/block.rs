use super::{
    InventoryType, header::Header, inventory::Inventory, transaction::Transaction, witness::Witness,
};
use neo_error::CoreResult;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};
use neo_storage::DataCache;
use serde::{Deserialize, Serialize};

/// Represents a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// The header of the block.
    pub header: Header,

    /// The transaction list of the block.
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Verify the merkle root matches the transactions.
    pub fn verify_merkle_root(&self) -> bool {
        if self.transactions.is_empty() {
            return self.header.merkle_root() == &UInt256::default();
        }
        let payload_hashes = match self.transaction_hashes() {
            Ok(h) => h,
            Err(_) => return false,
        };
        if let Some(root) = neo_crypto::MerkleTree::compute_root(&payload_hashes) {
            return self.header.merkle_root() == &root;
        }
        true
    }

    /// Verify no duplicate transactions.
    pub fn verify_no_duplicate_transactions(&self) -> bool {
        let mut seen: std::collections::HashSet<UInt256> = std::collections::HashSet::new();
        for tx in &self.transactions {
            let data = tx.hash_data();
            if data.len() != 32 {
                return false;
            }
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&data);
            let h = neo_primitives::UInt256::from_bytes(&bytes).unwrap_or_default();
            if !seen.insert(h) {
                return false;
            }
        }
        true
    }

    /// Creates a new block.
    /// Serialize without witnesses (delegates to header).
    pub fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Header as Serializable>::serialize(&self.header, writer)
    }

    /// Creates an empty block with a default header.
    pub fn new() -> Self {
        Self {
            header: Header::new(),
            transactions: Vec::new(),
        }
    }

    /// Creates a block from a header and its transactions. Replaces the former
    /// `ledger::Block::from_parts(header, transactions)`.
    pub fn from_parts(header: Header, transactions: Vec<Transaction>) -> Self {
        Self {
            header,
            transactions,
        }
    }

    /// Returns the block's primary (consensus) witness.
    pub fn primary_witness(&self) -> Option<&crate::Witness> {
        Some(&self.header.witness)
    }

    /// Gets the hash of the block.
    pub fn hash(&self) -> UInt256 {
        Header::hash(&self.header)
    }

    /// Gets the hash of the block, failing closed if the header cannot be
    /// serialized.
    pub fn try_hash(&self) -> CoreResult<UInt256> {
        self.header.try_hash()
    }

    /// Returns the unsigned header serialization used for block hashing.
    pub fn try_get_hash_data(&self) -> CoreResult<Vec<u8>> {
        self.header.try_get_hash_data()
    }

    /// Gets the version of the block.
    pub fn version(&self) -> u32 {
        self.header.version()
    }

    /// Gets the hash of the previous block.
    pub fn prev_hash(&self) -> &UInt256 {
        self.header.prev_hash()
    }

    /// Gets the merkle root of the transactions.
    pub fn merkle_root(&self) -> &UInt256 {
        self.header.merkle_root()
    }

    /// Gets the timestamp of the block.
    pub fn timestamp(&self) -> u64 {
        self.header.timestamp()
    }

    /// Gets the nonce of the block.
    pub fn nonce(&self) -> u64 {
        self.header.nonce()
    }

    /// Gets the index of the block.
    pub fn index(&self) -> u32 {
        self.header.index()
    }

    /// Gets the primary index of the consensus node.
    pub fn primary_index(&self) -> u8 {
        self.header.primary_index()
    }

    /// Gets the next consensus address.
    pub fn next_consensus(&self) -> &UInt160 {
        self.header.next_consensus()
    }

    /// Gets the witness of the block.
    pub fn witness(&self) -> &Witness {
        &self.header.witness
    }

    /// Calculates the network fee for the block.
    pub fn calculate_network_fee(&self, _snapshot: &DataCache) -> i64 {
        // Sum of all transaction network fees
        self.transactions.iter().map(|tx| tx.network_fee()).sum()
    }

    /// Rebuilds the merkle root.
    pub fn rebuild_merkle_root(&mut self) {
        if let Err(error) = self.try_rebuild_merkle_root() {
            tracing::error!(
                target: "neo::block",
                error = %error,
                "Failed to rebuild block merkle root"
            );
        }
    }

    /// Rebuilds the merkle root, failing closed if any transaction hash cannot
    /// be represented on the wire.
    pub fn try_rebuild_merkle_root(&mut self) -> CoreResult<()> {
        if self.transactions.is_empty() {
            self.header.set_merkle_root(UInt256::default());
            return Ok(());
        }
        let payload_hashes = self.transaction_hashes()?;
        if let Some(root) = neo_crypto::MerkleTree::compute_root(&payload_hashes) {
            self.header.set_merkle_root(root);
        }
        Ok(())
    }

    /// Returns the hashes of all transactions in the block, propagating any
    /// transaction-hash serialization failures.
    pub fn transaction_hashes(&self) -> CoreResult<Vec<UInt256>> {
        self.transactions.iter().map(|tx| tx.try_hash()).collect()
    }
}
impl neo_primitives::BlockLike for Block {
    type Transaction = Transaction;

    fn hash(&self) -> UInt256 {
        let clone = self.clone();
        clone.try_hash().unwrap_or_default()
    }

    fn index(&self) -> u32 {
        self.header.index()
    }

    fn timestamp(&self) -> u64 {
        self.header.timestamp()
    }

    fn prev_hash(&self) -> UInt256 {
        *self.header.prev_hash()
    }

    fn merkle_root(&self) -> UInt256 {
        *self.header.merkle_root()
    }

    fn transaction_count(&self) -> usize {
        self.transactions.len()
    }

    fn size(&self) -> usize {
        <Self as Serializable>::size(self)
    }
}

impl Inventory for Block {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Block
    }
}

impl crate::VerifiableExt for Block {}

impl neo_primitives::SerializablePayload for Block {
    fn hash_data(&self) -> Vec<u8> {
        self.header.hash_data()
    }

    fn witness_count(&self) -> usize {
        // Header witness + all transaction witnesses
        1 + self
            .transactions
            .iter()
            .map(|t| t.witness_count())
            .sum::<usize>()
    }

    fn invocation_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            return self.header.invocation_script(0);
        }
        let mut offset = 1;
        for tx in &self.transactions {
            let tx_count = tx.witness_count();
            if index < offset + tx_count {
                return tx.invocation_script(index - offset);
            }
            offset += tx_count;
        }
        &[]
    }

    fn verification_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            return self.header.verification_script(0);
        }
        let mut offset = 1;
        for tx in &self.transactions {
            let tx_count = tx.witness_count();
            if index < offset + tx_count {
                return tx.verification_script(index - offset);
            }
            offset += tx_count;
        }
        &[]
    }
}

// Use macro to reduce boilerplate
neo_io::impl_default_via_new!(Block);

impl neo_primitives::Verifiable for Block {
    fn hash(&self) -> neo_primitives::error::PrimitiveResult<neo_primitives::UInt256> {
        let data = self.header.try_get_hash_data().map_err(|e| {
            neo_primitives::error::PrimitiveError::invalid_data(format!(
                "block header serialization failed: {e}"
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn verify(&self) -> bool {
        true
    }
}

/// Wire size of a var-int (C# `GetVarSize`).
fn var_int_size(value: u64) -> usize {
    match value {
        v if v < 0xFD => 1,
        v if v <= 0xFFFF => 3,
        v if v <= 0xFFFF_FFFF => 5,
        _ => 9,
    }
}

impl Serializable for Block {
    fn size(&self) -> usize {
        // C# Block.Size includes the transaction var-array count bytes.
        let mut size = <Header as Serializable>::size(&self.header)
            + var_int_size(self.transactions.len() as u64);
        for tx in &self.transactions {
            size += <Transaction as Serializable>::size(tx);
        }
        size
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Header as Serializable>::serialize(&self.header, writer)?;
        writer.write_var_int(self.transactions.len() as u64)?;
        for tx in &self.transactions {
            <Transaction as Serializable>::serialize(tx, writer)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = <Header as neo_io::Serializable>::deserialize(reader)?;
        let tx_count = reader.read_var_int(usize::MAX as u64)? as usize;
        if tx_count > neo_primitives::constants::BLOCK_MAX_TX_WIRE_LIMIT {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        let mut transactions = Vec::with_capacity(tx_count);
        for _ in 0..tx_count {
            transactions.push(<Transaction as neo_io::Serializable>::deserialize(reader)?);
        }
        Ok(Self {
            header,
            transactions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::signer::Signer;
    use super::*;
    use neo_primitives::WitnessScope;
    use neo_vm_rs::OpCode;

    fn sample_header() -> Header {
        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(UInt256::from_bytes(&[1; 32]).expect("prev hash"));
        header.set_merkle_root(UInt256::from_bytes(&[2; 32]).expect("merkle root"));
        header.set_timestamp(1_700_000_000_000);
        header.set_nonce(0x0102_0304_0506_0708);
        header.set_index(42);
        header.set_primary_index(1);
        header.set_next_consensus(UInt160::from_bytes(&[3; 20]).expect("next consensus"));
        header.witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
        header
    }

    fn sample_block() -> Block {
        Block {
            header: sample_header(),
            transactions: Vec::new(),
        }
    }

    fn transaction_with_oversized_script() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(0x0102_0304);
        tx.set_system_fee(1);
        tx.set_network_fee(100_000_000);
        tx.set_valid_until_block(42);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    #[test]
    fn block_try_hash_delegates_to_header() {
        let block = sample_block();
        let header = block.header.clone();

        assert_eq!(
            block.try_hash().expect("block hash"),
            header.try_hash().unwrap()
        );
    }

    #[test]
    fn iverifiable_block_hash_uses_try_hash() {
        let block = sample_block();
        let expected_source = block.clone();
        let expected = expected_source.try_hash().expect("try hash");

        assert_eq!(
            <Block as neo_primitives::Verifiable>::hash(&block).unwrap(),
            expected
        );
    }

    #[test]
    fn block_try_get_hash_data_matches_header_hash_data() {
        let block = sample_block();

        assert_eq!(
            block.try_get_hash_data().expect("block hash data"),
            block.header.try_get_hash_data().expect("header hash data")
        );
    }

    #[test]
    fn try_rebuild_merkle_root_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());

        assert!(block.try_rebuild_merkle_root().is_err());
    }

    #[test]
    fn verify_merkle_root_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());
        block.header.set_merkle_root(UInt256::default());

        assert!(!block.verify_merkle_root());
    }

    #[test]
    fn duplicate_transaction_check_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());

        assert!(!block.verify_no_duplicate_transactions());
    }
}
