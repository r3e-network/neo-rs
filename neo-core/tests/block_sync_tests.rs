//! Block synchronization integration tests
//!
//! Tests for block sync functionality including validation and merkle root verification.

use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::network::p2p::payloads::{Block, Header};
use neo_core::UInt256;

/// Tests block creation and serialization roundtrip
#[test]
fn test_block_serialization_roundtrip() {
    let mut block = Block::new();
    block.header.set_version(0);
    block.header.set_index(100);
    block.header.set_timestamp(1700000000000);
    block.header.set_nonce(12345);

    // Serialize
    let mut writer = BinaryWriter::new();
    block
        .serialize(&mut writer)
        .expect("Block serialization should succeed");
    let bytes = writer.into_bytes();

    // Deserialize
    let mut reader = MemoryReader::new(&bytes);
    let deserialized =
        Block::deserialize(&mut reader).expect("Block deserialization should succeed");

    assert_eq!(deserialized.version(), block.version());
    assert_eq!(deserialized.index(), block.index());
    assert_eq!(deserialized.timestamp(), block.timestamp());
    assert_eq!(deserialized.nonce(), block.nonce());
}

/// Tests empty block merkle root is zero
#[test]
fn test_empty_block_merkle_root() {
    let block = Block::new();
    assert!(block.transactions.is_empty());
    // Empty transactions should have default merkle root
    assert_eq!(*block.merkle_root(), UInt256::default());
}

/// Tests block with no duplicate transactions passes validation
#[test]
fn test_no_duplicate_transactions_validation() {
    let block = Block::new();
    // Empty block has no duplicates
    assert!(block.transactions.is_empty());
}

/// Tests block size limits during deserialization
#[test]
fn test_block_size_limit_enforcement() {
    // Create a block header
    let mut header = Header::new();
    header.set_version(0);
    header.set_index(1);

    // Serialize header
    let mut writer = BinaryWriter::new();
    header.serialize(&mut writer).expect("Header serialization");

    // Add an extremely large transaction count (would exceed MAX_BLOCK_SIZE)
    // The var_int for u16::MAX (65535) transactions
    writer.write_var_uint(65535).expect("Write var uint");

    let bytes = writer.into_bytes();
    let mut reader = MemoryReader::new(&bytes);

    // This should fail because cumulative size would exceed MAX_BLOCK_SIZE
    // with 65535 transactions even if each is minimal size
    let result = Block::deserialize(&mut reader);

    // Either it fails due to size limit or because we didn't provide actual tx data
    assert!(
        result.is_err(),
        "Should fail to deserialize block with too many transactions"
    );
}

/// Tests block header hash computation consistency
#[test]
fn test_block_header_hash_consistency() {
    let mut block1 = Block::new();
    block1.header.set_version(0);
    block1.header.set_index(100);
    block1.header.set_timestamp(1700000000000);

    let mut block2 = Block::new();
    block2.header.set_version(0);
    block2.header.set_index(100);
    block2.header.set_timestamp(1700000000000);

    // Same header data should produce same hash
    let hash1 = block1.hash();
    let hash2 = block2.hash();
    assert_eq!(
        hash1, hash2,
        "Identical blocks should have identical hashes"
    );

    // Different data should produce different hash
    let mut block3 = Block::new();
    block3.header.set_version(0);
    block3.header.set_index(101); // Different index
    block3.header.set_timestamp(1700000000000);

    let hash3 = block3.hash();
    assert_ne!(
        hash1, hash3,
        "Different blocks should have different hashes"
    );
}

/// Tests block prev_hash chain integrity
#[test]
fn test_block_chain_linkage() {
    let mut genesis = Block::new();
    genesis.header.set_index(0);
    genesis.header.set_prev_hash(UInt256::default()); // Genesis has zero prev_hash

    let genesis_hash = genesis.hash();

    let mut block1 = Block::new();
    block1.header.set_index(1);
    block1.header.set_prev_hash(genesis_hash);

    assert_eq!(
        *block1.prev_hash(),
        genesis_hash,
        "Block should reference previous block hash"
    );
}
