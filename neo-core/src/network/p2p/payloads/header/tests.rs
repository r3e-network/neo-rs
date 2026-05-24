use super::*;
use crate::ledger::block_header::BlockHeader as LedgerBlockHeader;
use crate::ledger::HeaderCache;
use crate::neo_io::{BinaryWriter, Serializable};
use crate::persistence::i_store::IStore;
use crate::persistence::providers::memory_store::MemoryStore;
use crate::persistence::StoreCache;
use crate::smart_contract::native::trimmed_block::TrimmedBlock;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::StorageItem;
use crate::Witness as LedgerWitness;
use neo_vm_rs::OpCode;
use std::sync::Arc;

const LEDGER_CONTRACT_ID: i32 = -4;

fn sample_witness() -> Witness {
    Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()])
}

fn sample_settings() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default_settings();
    settings.validators_count = 7;
    settings
}

fn sample_header_for_hashing() -> Header {
    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(UInt256::from_bytes(&[1; 32]).expect("prev hash"));
    header.set_merkle_root(UInt256::from_bytes(&[2; 32]).expect("merkle root"));
    header.set_timestamp(1_700_000_000_000);
    header.set_nonce(0x0102_0304_0506_0708);
    header.set_index(42);
    header.set_primary_index(1);
    header.set_next_consensus(UInt160::from_bytes(&[3; 20]).expect("next consensus"));
    header.witness = sample_witness();
    header
}

#[test]
fn try_hash_matches_legacy_hash_for_valid_header() {
    let mut header = sample_header_for_hashing();
    let mut legacy_header = header.clone();

    assert_eq!(header.try_hash().expect("try hash"), legacy_header.hash());
}

#[test]
fn try_get_hash_data_matches_serialize_unsigned() {
    let header = sample_header_for_hashing();
    let mut writer = BinaryWriter::new();
    header
        .serialize_unsigned(&mut writer)
        .expect("serialize unsigned");

    assert_eq!(
        header.try_get_hash_data().expect("hash data"),
        writer.into_bytes()
    );
}

#[test]
fn iverifiable_header_hash_uses_try_hash() {
    let header = sample_header_for_hashing();
    let mut expected_source = header.clone();
    let expected = expected_source.try_hash().expect("try hash");

    assert_eq!(
        <Header as crate::IVerifiable>::hash(&header).unwrap(),
        expected
    );
}

fn insert_trimmed_block(store_cache: &mut StoreCache, header: &Header, block_hash: UInt256) {
    let ledger_witness = LedgerWitness::new_with_scripts(
        header.witness.invocation_script.clone(),
        header.witness.verification_script.clone(),
    );

    let ledger_header = LedgerBlockHeader::new(
        header.version(),
        *header.prev_hash(),
        *header.merkle_root(),
        header.timestamp(),
        header.nonce(),
        header.index(),
        header.primary_index(),
        *header.next_consensus(),
        vec![ledger_witness],
    );

    let trimmed = TrimmedBlock::create(ledger_header, Vec::new());
    let mut writer = BinaryWriter::new();
    Serializable::serialize(&trimmed, &mut writer).expect("serialize trimmed block");
    let payload = writer.into_bytes();

    let mut key = Vec::with_capacity(1 + block_hash.to_bytes().len());
    key.push(5); // PREFIX_BLOCK
    key.extend_from_slice(&block_hash.to_bytes());

    store_cache.add(
        StorageKey::new(LEDGER_CONTRACT_ID, key),
        StorageItem::from_bytes(payload),
    );
}

#[test]
fn verify_with_cache_succeeds_for_sequential_header() {
    use crate::validation::MIN_TIMESTAMP_MS;

    let mut prev_header = Header::new();
    prev_header.set_version(0);
    prev_header.set_index(0);
    prev_header.set_timestamp(MIN_TIMESTAMP_MS + 1_000);
    prev_header.set_primary_index(0);

    let deterministic_witness =
        Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT.byte(), OpCode::RET.byte()]);
    let consensus = deterministic_witness.script_hash();
    prev_header.witness = deterministic_witness.clone();
    prev_header.set_next_consensus(consensus);

    let mut prev_clone = prev_header.clone();
    let prev_hash = prev_clone.hash();

    let cache = HeaderCache::new();
    cache.add(prev_header.clone());

    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(prev_hash);
    header.set_index(1);
    header.set_timestamp(MIN_TIMESTAMP_MS + 2_000);
    header.set_primary_index(0);
    header.witness = deterministic_witness;

    let settings = sample_settings();
    let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(store, false);

    assert!(header.verify_with_cache(&settings, &store_cache, &cache));
}

#[test]
fn verify_with_cache_rejects_when_timestamp_not_increasing() {
    use crate::validation::MIN_TIMESTAMP_MS;

    let mut prev_header = Header::new();
    prev_header.set_version(0);
    prev_header.set_index(10);
    prev_header.set_timestamp(MIN_TIMESTAMP_MS + 5_000);
    prev_header.set_primary_index(0);

    let prev_witness = sample_witness();
    let consensus = prev_witness.script_hash();
    prev_header.witness = prev_witness;
    prev_header.set_next_consensus(consensus);

    let mut prev_clone = prev_header.clone();
    let prev_hash = prev_clone.hash();

    let cache = HeaderCache::new();
    cache.add(prev_header.clone());

    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(prev_hash);
    header.set_index(11);
    header.set_timestamp(MIN_TIMESTAMP_MS + 5_000); // not strictly greater
    header.set_primary_index(0);
    header.witness = sample_witness();

    let settings = sample_settings();
    let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(store, true);

    assert!(!header.verify_with_cache(&settings, &store_cache, &cache));
}

#[test]
fn verify_uses_persisted_state_when_cache_empty() {
    use crate::validation::MIN_TIMESTAMP_MS;

    let mut prev_header = Header::new();
    prev_header.set_version(0);
    prev_header.set_index(20);
    prev_header.set_timestamp(MIN_TIMESTAMP_MS + 7_500);
    prev_header.set_primary_index(0);

    let prev_witness = sample_witness();
    let consensus = prev_witness.script_hash();
    prev_header.witness = prev_witness;
    prev_header.set_next_consensus(consensus);

    let mut prev_clone = prev_header.clone();
    let prev_hash = prev_clone.hash();

    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(prev_hash);
    header.set_index(21);
    header.set_timestamp(MIN_TIMESTAMP_MS + 8_000);
    header.set_primary_index(0);
    header.witness = sample_witness();

    let settings = sample_settings();
    let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
    let mut store_cache = StoreCache::new_from_store(store, false);
    insert_trimmed_block(&mut store_cache, &prev_header, prev_hash);

    let ledger = LedgerContract::new();
    let prev_trimmed = ledger
        .get_trimmed_block(&store_cache, &prev_hash)
        .expect("trimmed block lookup")
        .expect("trimmed block should exist");

    let validation = header.validate_against_previous(
        &settings,
        prev_trimmed.index(),
        &prev_trimmed.hash(),
        prev_trimmed.header.timestamp,
    );
    assert!(
        validation.is_ok(),
        "validation failed: {:?}",
        validation.err()
    );

    let verified = header.verify_witness_against_hash(
        &settings,
        store_cache.data_cache(),
        &prev_trimmed.header.next_consensus,
        &header.witness,
        HEADER_VERIFY_GAS,
    );
    assert!(verified);

    assert!(header.verify(&settings, &store_cache));
}
