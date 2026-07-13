use super::*;
use crate::signer::Signer;
use neo_crypto::Crypto;
use neo_primitives::WitnessScope;
use neo_vm::OpCode;

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

fn valid_transaction(nonce: u32, signer_seed: u8) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(nonce);
    tx.set_system_fee(1);
    tx.set_network_fee(100_000_000);
    tx.set_valid_until_block(42);
    tx.set_signers(vec![Signer::new(
        UInt160::from_bytes(&[signer_seed; 20]).expect("signer"),
        WitnessScope::NONE,
    )]);
    tx.set_attributes(Vec::new());
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn serialize_block(block: &Block) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    <Block as Serializable>::serialize(block, &mut writer).expect("serialize block");
    writer.into_bytes()
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
fn block_hash_is_single_sha256_of_unsigned_header_data() {
    let block = sample_block();
    let unsigned = block.try_get_hash_data().expect("block hash data");
    let first_digest = Crypto::sha256(&unsigned);
    let second_digest = Crypto::sha256(&first_digest);
    let expected_single = UInt256::from(first_digest);

    assert_eq!(block.try_hash().expect("block hash"), expected_single);
    assert_eq!(
        <Block as neo_primitives::SerializablePayload>::hash(&block),
        expected_single
    );
    assert_ne!(
        block.try_hash().expect("block hash"),
        UInt256::from(second_digest),
        "C# Block.IVerifiable.SerializeUnsigned delegates to Header.SerializeUnsigned and Helper.CalculateHash applies one SHA256"
    );
}

#[test]
fn verifiable_block_hash_data_matches_header_unsigned_hash_data() {
    let block = sample_block();
    let full_header_bytes = {
        let mut writer = BinaryWriter::new();
        <Header as Serializable>::serialize(&block.header, &mut writer)
            .expect("serialize full header");
        writer.into_bytes()
    };

    assert_eq!(
        <Block as neo_primitives::Verifiable>::hash_data(&block),
        block.header.try_get_hash_data().expect("header hash data"),
        "C# Block.IVerifiable.SerializeUnsigned delegates to Header.IVerifiable.SerializeUnsigned"
    );
    assert_ne!(
        <Block as neo_primitives::Verifiable>::hash_data(&block),
        full_header_bytes,
        "block hash data must not include the header witness"
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

#[test]
fn duplicate_transaction_check_uses_transaction_hashes() {
    let mut distinct = sample_block();
    distinct.transactions.push(valid_transaction(1, 1));
    distinct.transactions.push(valid_transaction(2, 2));
    assert!(distinct.verify_no_duplicate_transactions());

    let tx = valid_transaction(3, 3);
    let mut duplicate = sample_block();
    duplicate.transactions.push(tx.clone());
    duplicate.transactions.push(tx);
    assert!(!duplicate.verify_no_duplicate_transactions());
}

#[test]
fn deserialize_rejects_duplicate_transaction_hashes_like_csharp() {
    let tx = valid_transaction(7, 7);
    let mut block = sample_block();
    block.transactions.push(tx.clone());
    block.transactions.push(tx);
    block.try_rebuild_merkle_root().expect("merkle root");

    let bytes = serialize_block(&block);
    let mut reader = MemoryReader::new(&bytes);
    let err = <Block as Serializable>::deserialize(&mut reader)
        .expect_err("C# Block.Deserialize rejects duplicate transaction hashes");
    assert!(
        err.to_string().contains("duplicate transaction"),
        "unexpected error: {err}"
    );
}

#[test]
fn deserialize_rejects_merkle_root_mismatch_like_csharp() {
    let mut block = sample_block();
    block.transactions.push(valid_transaction(8, 8));
    block.header.set_merkle_root(UInt256::zero());

    let bytes = serialize_block(&block);
    let mut reader = MemoryReader::new(&bytes);
    let err = <Block as Serializable>::deserialize(&mut reader)
        .expect_err("C# Block.Deserialize rejects mismatched Merkle roots");
    assert!(
        err.to_string().contains("Merkle root"),
        "unexpected error: {err}"
    );
}
