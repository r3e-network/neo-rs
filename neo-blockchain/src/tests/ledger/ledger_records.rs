use super::*;
use neo_payloads::Header;

/// The keys built here must parse through the Rust `LedgerContract`
/// readers — that is the whole point of writing via its codec.
#[test]
fn records_round_trip_through_the_ledger_contract_readers() {
    let cache = DataCache::new(false);
    let ledger = LedgerContract::new();

    let mut header = Header::new();
    header.set_index(7);
    let mut tx = Transaction::new();
    tx.set_nonce(99);
    tx.set_script(vec![0x40]); // RET
    tx.set_signers(vec![neo_payloads::Signer::new(
        UInt160::from_bytes(&[0x22; 20]).unwrap(),
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = Block::from_parts(header, vec![tx.clone()]);
    let block_hash = block.header.try_hash().expect("block hash");

    LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
        .expect("on-persist records");
    LedgerRecords::write_post_persist_record(&cache, &block_hash, 7).expect("post-persist record");

    // Block-hash index + trimmed block.
    assert_eq!(
        ledger.get_block_hash(&cache, 7).expect("get_block_hash"),
        Some(block_hash)
    );
    let trimmed = ledger
        .get_trimmed_block(&cache, &block_hash)
        .expect("get_trimmed_block")
        .expect("trimmed block present");
    assert_eq!(trimmed.header.index(), 7);
    assert_eq!(trimmed.hashes, vec![tx_hash]);

    // Transaction record: initial NONE state, then the post-execution
    // rewrite flips it to HALT.
    let state = ledger
        .get_transaction_state(&cache, &tx_hash)
        .expect("get_transaction_state")
        .expect("record present");
    assert_eq!(state.block_index, 7);
    assert_eq!(state.state, VMState::NONE);
    LedgerRecords::update_transaction_vm_state(&cache, 7, &tx, &tx_hash, VMState::HALT)
        .expect("vm-state rewrite");
    let state = ledger
        .get_transaction_state(&cache, &tx_hash)
        .expect("get_transaction_state")
        .expect("record present");
    assert_eq!(state.state, VMState::HALT);

    // Current-block pointer.
    assert_eq!(ledger.current_index(&cache).expect("current_index"), 7);
    assert_eq!(
        ledger.current_hash(&cache).expect("current_hash"),
        block_hash
    );
}

/// Byte-level pin of the C# `LedgerContract.OnPersistAsync` /
/// `PostPersistAsync` write set: every key (prefix + big-endian
/// index / raw hash) and every value (raw hash bytes, TrimmedBlock
/// `ISerializable` bytes, `BinarySerializer` stack-item records)
/// is asserted against independently assembled C# layouts — not
/// just round-tripped through the Rust codec.
#[test]
fn persisted_records_pin_csharp_key_and_value_bytes() {
    let cache = DataCache::new(false);

    let mut header = Header::new();
    header.set_index(0x0102_0304);
    let mut tx = Transaction::new();
    tx.set_nonce(7);
    tx.set_script(vec![0x40]); // RET
    tx.set_signers(vec![neo_payloads::Signer::new(
        UInt160::from_bytes(&[0x22; 20]).unwrap(),
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let tx_hash = tx.try_hash().unwrap();
    let block = Block::from_parts(header, vec![tx.clone()]);
    let block_hash = block.header.try_hash().unwrap();

    LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
        .expect("on-persist records");
    LedgerRecords::write_post_persist_record(&cache, &block_hash, 0x0102_0304)
        .expect("post-persist");

    let raw = |key: &StorageKey| {
        cache
            .get(key)
            .map(|item| item.value_bytes().into_owned())
            .expect("record present")
    };

    // --- Prefix_BlockHash (9): key = prefix ‖ BIG-ENDIAN index;
    // value = the raw 32-byte block hash.
    let bh_key = LedgerRecords::block_hash_key(0x0102_0304);
    assert_eq!(bh_key.id(), LedgerContract::ID);
    assert_eq!(bh_key.key(), &[9u8, 0x01, 0x02, 0x03, 0x04]);
    assert_eq!(raw(&bh_key), block_hash.to_bytes());

    // --- Prefix_Block (5): key = prefix ‖ hash; value =
    // TrimmedBlock.ToArray() = header bytes ‖ var-int count ‖ hashes.
    let b_key = LedgerRecords::block_key(&block_hash);
    let mut expected_key = vec![5u8];
    expected_key.extend_from_slice(&block_hash.to_bytes());
    assert_eq!(b_key.key(), &expected_key[..]);
    let mut header_writer = neo_io::BinaryWriter::new();
    Serializable::serialize(&block.header, &mut header_writer).unwrap();
    let mut expected_block = header_writer.into_bytes();
    expected_block.push(1); // var-int tx count
    expected_block.extend_from_slice(&tx_hash.to_bytes());
    assert_eq!(raw(&b_key), expected_block);

    // --- Prefix_Transaction (11): value = BinarySerializer bytes of
    // Struct[Integer(BlockIndex), ByteString(tx bytes), Integer(state)]
    // (0x41 Struct, 0x21 Integer, 0x28 ByteString; VMState.NONE = 0
    // serializes as the empty Integer span).
    let mut tx_writer = neo_io::BinaryWriter::new();
    Serializable::serialize(&tx, &mut tx_writer).unwrap();
    let tx_bytes = tx_writer.into_bytes();
    assert!(tx_bytes.len() < 0xFD);
    let mut expected_record = vec![
        0x41,
        0x03,
        0x21,
        0x04,
        0x04,
        0x03,
        0x02,
        0x01, // Integer 0x01020304 LE
        0x28,
        tx_bytes.len() as u8,
    ];
    expected_record.extend_from_slice(&tx_bytes);
    expected_record.extend_from_slice(&[0x21, 0x00]); // VMState::NONE
    assert_eq!(
        raw(&LedgerRecords::transaction_key(&tx_hash)),
        expected_record
    );

    // After execution the record is rewritten with HALT (= 1).
    LedgerRecords::update_transaction_vm_state(&cache, 0x0102_0304, &tx, &tx_hash, VMState::HALT)
        .unwrap();
    let mut expected_halt = vec![
        0x41,
        0x03,
        0x21,
        0x04,
        0x04,
        0x03,
        0x02,
        0x01,
        0x28,
        tx_bytes.len() as u8,
    ];
    expected_halt.extend_from_slice(&tx_bytes);
    expected_halt.extend_from_slice(&[0x21, 0x01, 0x01]);
    assert_eq!(
        raw(&LedgerRecords::transaction_key(&tx_hash)),
        expected_halt
    );

    // --- Prefix_CurrentBlock (12): value = BinarySerializer bytes of
    // Struct[ByteString(hash), Integer(index)] (HashIndexState).
    assert_eq!(LedgerRecords::current_block_key().key(), &[12u8]);
    let mut expected_pointer = vec![0x41, 0x02, 0x28, 0x20];
    expected_pointer.extend_from_slice(&block_hash.to_bytes());
    expected_pointer.extend_from_slice(&[0x21, 0x04, 0x04, 0x03, 0x02, 0x01]);
    assert_eq!(raw(&LedgerRecords::current_block_key()), expected_pointer);
}

/// C# stores conflict stubs under the bare conflict hash and under
/// hash‖signer; the bare-hash stub must read back as a record whose
/// `transaction` is `None`.
#[test]
fn conflict_attributes_write_stub_records() {
    let cache = DataCache::new(false);
    let ledger = LedgerContract::new();

    let conflict_hash = UInt256::from_bytes(&[0xAB; 32]).unwrap();
    let signer_account = UInt160::from_bytes(&[0x11; 20]).unwrap();
    let mut tx = Transaction::new();
    tx.set_script(vec![0x40]);
    tx.set_signers(vec![neo_payloads::Signer::new(
        signer_account,
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_attributes(vec![TransactionAttribute::Conflicts(
        neo_payloads::Conflicts::new(conflict_hash),
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let mut header = Header::new();
    header.set_index(3);
    let block = Block::from_parts(header, vec![tx]);
    let block_hash = block.header.try_hash().unwrap();

    LedgerRecords::write_on_persist_records(&cache, &block, &block_hash).expect("records");

    let stub = ledger
        .get_transaction_state(&cache, &conflict_hash)
        .expect("read stub")
        .expect("stub present");
    assert!(
        stub.transaction.is_none(),
        "conflict stub has no transaction"
    );
    assert_eq!(stub.block_index, 3);

    // The signer-suffixed stub exists with the same payload.
    let key = LedgerRecords::conflict_signer_key(&conflict_hash, &signer_account);
    let raw = cache.get(&key).expect("signer stub present");
    assert_eq!(
        raw.value_bytes().into_owned(),
        LedgerContract::new().serialize_conflict_stub(3).unwrap()
    );
}
