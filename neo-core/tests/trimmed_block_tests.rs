use chrono::{TimeZone, Utc};
use neo_core::UInt160;
use neo_core::UInt256;
use neo_core::Witness;
use neo_core::ledger::Block;
use neo_core::ledger::block_header::BlockHeader;
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::smart_contract::native::trimmed_block::TrimmedBlock;

fn sample_timestamp() -> u64 {
    Utc.with_ymd_and_hms(1988, 6, 1, 0, 0, 0)
        .unwrap()
        .timestamp() as u64
}

fn sample_witness() -> Witness {
    Witness::new_with_scripts(Vec::new(), vec![neo_vm::op_code::OpCode::PUSH1 as u8])
}

fn trimmed_block_with_no_transactions() -> TrimmedBlock {
    let header = BlockHeader::new(
        0,
        UInt256::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff01")
            .unwrap(),
        UInt256::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff02")
            .unwrap(),
        sample_timestamp(),
        0,
        1,
        0,
        UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").unwrap(),
        vec![sample_witness()],
    );
    TrimmedBlock::create(header, Vec::new())
}

fn transaction_with_tail(byte: u8) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_script(vec![byte; 4]);
    tx
}

#[test]
fn trimmed_block_header_fields_match() {
    let block = trimmed_block_with_no_transactions();
    assert_eq!(
        block.header.previous_hash,
        UInt256::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff01")
            .unwrap()
    );
    assert_eq!(
        block.header.merkle_root,
        UInt256::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff02")
            .unwrap()
    );
    assert_eq!(block.index(), 1);
}

#[test]
fn trimmed_block_size_matches_reference() {
    let mut block = trimmed_block_with_no_transactions();
    block.hashes = vec![
        UInt256::parse("0x33d3b8965712d1c1d9edb1e9f5bdc8dfeadfde7d572bea3522eef19aef2da56d")
            .unwrap(),
    ];
    assert_eq!(block.size(), 146);
}

#[test]
fn trimmed_block_clone_produces_independent_copy() {
    let mut original = trimmed_block_with_no_transactions();
    original.hashes = vec![
        UInt256::parse("0x22d3b8965712d1c1d9edb1e9f5bdc8dfeadfde7d572bea3522eef19aef2da56c")
            .unwrap(),
    ];

    let mut clone = original.clone();
    clone.header.index += 1;

    let mut writer = BinaryWriter::new();
    original.serialize(&mut writer).expect("serialize original");
    let original_bytes = writer.into_bytes();

    let mut writer = BinaryWriter::new();
    clone.serialize(&mut writer).expect("serialize clone");
    let clone_bytes = writer.into_bytes();

    assert_ne!(original_bytes, clone_bytes);
    assert_ne!(original.header.index, clone.header.index);
}

#[test]
fn trimmed_block_serialization_roundtrips() {
    let mut block = trimmed_block_with_no_transactions();
    block.hashes = vec![
        UInt256::parse("0x1111111111111111111111111111111111111111111111111111111111111111")
            .unwrap(),
    ];

    let mut writer = BinaryWriter::new();
    block.serialize(&mut writer).expect("serialize block");
    let bytes = writer.into_bytes();

    let mut reader = MemoryReader::new(&bytes);
    let deserialized = TrimmedBlock::deserialize(&mut reader).expect("deserialize block");

    assert_eq!(deserialized.hash(), block.hash());
    assert_eq!(deserialized.hashes(), block.hashes());
    assert_eq!(
        deserialized.header.next_consensus,
        block.header.next_consensus
    );
}

#[test]
fn trimmed_block_from_block_collects_transaction_hashes() {
    let header = BlockHeader::new(
        0,
        UInt256::parse("0x1000000000000000000000000000000000000000000000000000000000000000")
            .unwrap(),
        UInt256::parse("0x2000000000000000000000000000000000000000000000000000000000000000")
            .unwrap(),
        sample_timestamp(),
        0,
        42,
        3,
        UInt160::parse("0xa400ff00ff00ff00ff00ff00ff00ff00ff00ff01").unwrap(),
        vec![sample_witness()],
    );

    let tx1 = transaction_with_tail(0x01);
    let mut tx2 = transaction_with_tail(0x02);
    tx2.set_nonce(1234);

    let block = Block::new(header.clone(), vec![tx1.clone(), tx2.clone()]);

    let trimmed = TrimmedBlock::from_block(&block);
    let hashes: Vec<UInt256> = vec![tx1.hash(), tx2.hash()];

    assert_eq!(trimmed.header.hash(), header.hash());
    assert_eq!(trimmed.index(), 42);
    assert_eq!(trimmed.hashes(), hashes.as_slice());
}
