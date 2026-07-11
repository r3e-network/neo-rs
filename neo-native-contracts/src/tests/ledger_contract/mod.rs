//! # neo-native-contracts::tests::ledger_contract
//!
//! Test module grouping Native Ledger contract storage and query behavior.
//! coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::wire::HashIndexState;
use super::*;
use neo_execution::Interoperable;
use neo_io::{BinaryWriter, Serializable};
use neo_payloads::Transaction;
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::StackItem;
use neo_vm_rs::{ExecutionEngineLimits, StackValue, VmState as VMState};

/// Structural equality for StackValue that ignores the reference-identity ids
/// on compound variants. Collection identity is not part of serialized
/// stack data, so structural equality is the correct notion for round-trip / shape
/// assertions.
fn stack_value_struct_eq(a: &neo_vm_rs::StackValue, b: &neo_vm_rs::StackValue) -> bool {
    use neo_vm_rs::StackValue::*;
    match (a, b) {
        (Buffer(x), Buffer(y)) => x == y,
        (Array(x), Array(y)) | (Struct(x), Struct(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| stack_value_struct_eq(p, q))
        }
        (Map(x), Map(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((k1, v1), (k2, v2))| {
                    stack_value_struct_eq(k1, k2) && stack_value_struct_eq(v1, v2)
                })
        }
        _ => a == b,
    }
}

#[test]
fn native_contract_surface() {
    let c = LedgerContract::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "currentHash",
            "currentIndex",
            "getTransactionHeight",
            "getTransactionVMState",
            "getTransaction",
            "getTransactionSigners",
            "getBlock",
            "getTransactionFromBlock"
        ]
    );
    assert!(
        c.methods()
            .iter()
            .all(|m| m.safe && m.required_call_flags == CallFlags::READ_STATES.bits())
    );
    for name in ["getTransactionHeight", "getTransactionVMState"] {
        let m = c.methods().iter().find(|m| m.name == name).unwrap();
        assert_eq!(m.parameters, vec![ContractParameterType::Hash256]);
        assert_eq!(m.return_type, ContractParameterType::Integer);
        assert_eq!(m.cpu_fee, 1 << 15);
    }
    for name in ["getTransaction", "getTransactionSigners"] {
        let m = c.methods().iter().find(|m| m.name == name).unwrap();
        assert_eq!(m.parameters, vec![ContractParameterType::Hash256]);
        assert_eq!(m.return_type, ContractParameterType::Array);
        assert_eq!(m.cpu_fee, 1 << 15);
    }
    // getBlock takes a single ByteArray (indexOrHash) and returns an Array.
    let get_block = c.methods().iter().find(|m| m.name == "getBlock").unwrap();
    assert_eq!(get_block.parameters, vec![ContractParameterType::ByteArray]);
    assert_eq!(get_block.return_type, ContractParameterType::Array);
    assert_eq!(get_block.cpu_fee, 1 << 15);
    // getTransactionFromBlock takes (ByteArray, Integer) and is the only
    // ledger read with the heavier 1 << 16 CPU fee.
    let from_block = c
        .methods()
        .iter()
        .find(|m| m.name == "getTransactionFromBlock")
        .unwrap();
    assert_eq!(
        from_block.parameters,
        vec![
            ContractParameterType::ByteArray,
            ContractParameterType::Integer
        ]
    );
    assert_eq!(from_block.return_type, ContractParameterType::Array);
    assert_eq!(from_block.cpu_fee, 1 << 16);
}

#[test]
fn block_lookup_args_use_shared_raw_integer_helpers() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let query_source = include_str!("../../ledger_contract/queries.rs");
    let resolver = slice_between(
        query_source,
        "fn resolve_block_hash",
        "fn is_traceable_block",
    );
    assert!(resolver.contains("crate::args::raw_integer_bytes_to_u32"));
    assert!(!resolver.contains("BigInt::from_signed_bytes_le(index_or_hash"));

    let invoke_source = include_str!("../../ledger_contract/invoke.rs");
    let get_block = slice_between(
        invoke_source,
        "fn invoke_get_block",
        "fn invoke_get_transaction_from_block",
    );
    assert!(get_block.contains("crate::args::raw_arg"));
    assert!(!get_block.contains("args.first()"));

    let from_block_start = invoke_source
        .find("fn invoke_get_transaction_from_block")
        .expect("getTransactionFromBlock handler exists");
    let from_block = &invoke_source[from_block_start..];
    assert!(from_block.contains("crate::args::raw_arg"));
    assert!(from_block.contains("crate::args::raw_integer_bytes_to_i32"));
    assert!(!from_block.contains("BigInt::from_signed_bytes_le(tx_index_bytes"));
}

#[test]
fn get_trimmed_block_round_trips_through_storage() {
    use neo_payloads::{Header, TrimmedBlock};

    let cache = DataCache::new(false);
    let ledger = LedgerContract::new();

    let mut header = Header::new();
    header.set_index(77);
    header.set_nonce(u64::MAX);
    let block_hash = header.hash();

    let trimmed = TrimmedBlock::new(
        header,
        vec![
            UInt256::from_bytes(&[0x11u8; 32]).unwrap(),
            UInt256::from_bytes(&[0x22u8; 32]).unwrap(),
        ],
    );

    // Absent block -> None.
    assert!(
        ledger
            .get_trimmed_block(&cache, &block_hash)
            .unwrap()
            .is_none()
    );

    // Persist the trimmed block exactly as OnPersist does
    // (TrimmedBlock.ToArray() = ISerializable bytes) and read it back.
    let mut writer = BinaryWriter::new();
    trimmed.serialize(&mut writer).unwrap();
    cache.add(
        LedgerContract::block_storage_key(&block_hash),
        StorageItem::from_bytes(writer.into_bytes()),
    );

    let loaded = ledger
        .get_trimmed_block(&cache, &block_hash)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.header.index(), 77);
    assert_eq!(loaded.header.nonce(), u64::MAX);
    assert_eq!(loaded.hashes, trimmed.hashes);
}

#[test]
fn resolve_block_hash_handles_index_hash_and_bad_length() {
    let cache = DataCache::new(false);
    let ledger = LedgerContract::new();

    // Exactly 32 bytes: the argument is the hash itself.
    let raw = [0x5Au8; 32];
    assert_eq!(
        ledger.resolve_block_hash(&cache, &raw).unwrap(),
        Some(UInt256::from_bytes(&raw).unwrap())
    );

    // Fewer than 32 bytes: a block index resolved via the block-hash index.
    // Absent index -> None.
    assert_eq!(ledger.resolve_block_hash(&cache, &[5u8]).unwrap(), None);
    let indexed_hash = UInt256::from_bytes(&[0x7u8; 32]).unwrap();
    cache.add(
        LedgerContract::block_hash_storage_key(5),
        StorageItem::from_bytes(indexed_hash.to_bytes()),
    );
    assert_eq!(
        ledger.resolve_block_hash(&cache, &[5u8]).unwrap(),
        Some(indexed_hash)
    );

    // More than 32 bytes: rejected (C# ArgumentException).
    assert!(ledger.resolve_block_hash(&cache, &[0u8; 33]).is_err());
}

#[test]
fn trace_window_matches_csharp_is_traceable_block() {
    // current=100, mtb=10 => traceable indices are (90, 100].
    // Future block: never traceable.
    assert!(!LedgerContract::is_within_trace_window(101, 100, 10));
    // Lower boundary is exclusive: index + mtb must be strictly > current.
    // index=90 -> 90+10=100, not > 100 -> not traceable.
    assert!(!LedgerContract::is_within_trace_window(90, 100, 10));
    // index=91 -> 101 > 100 -> traceable; current index is traceable.
    assert!(LedgerContract::is_within_trace_window(91, 100, 10));
    assert!(LedgerContract::is_within_trace_window(100, 100, 10));
    // Genesis is traceable at genesis for any positive window.
    assert!(LedgerContract::is_within_trace_window(0, 0, 2_102_400));
}

#[test]
fn get_transaction_state_distinguishes_absent_stub_and_full() {
    let cache = DataCache::new(false);
    let ledger = LedgerContract::new();
    let tx_hash = UInt256::from_bytes(&[9u8; 32]).unwrap();

    // Absent -> None (getTransactionHeight would return -1).
    assert!(
        ledger
            .get_transaction_state(&cache, &tx_hash)
            .unwrap()
            .is_none()
    );
    assert!(!ledger.contains_transaction(&cache, &tx_hash).unwrap());

    // Conflict stub -> Some, but `transaction` is None, so C#
    // `GetTransactionState` treats it as null and height is -1 —
    // and C# `ContainsTransaction` is false for a stub.
    cache.add(
        LedgerContract::transaction_storage_key(&tx_hash),
        StorageItem::from_bytes(LedgerContract::new().serialize_conflict_stub(4242).unwrap()),
    );
    let stub = ledger
        .get_transaction_state(&cache, &tx_hash)
        .unwrap()
        .unwrap();
    assert!(stub.transaction.is_none());
    assert_eq!(stub.block_index, 4242);
    assert!(
        !ledger.contains_transaction(&cache, &tx_hash).unwrap(),
        "C# ContainsTransaction must be false for a conflict stub"
    );
}

/// Byte-level pins of the C# `KeyBuilder` key layouts:
/// `CreateStorageKey(Prefix_BlockHash, uint)` uses `AddBigEndian`
/// (NativeContract.cs:403), and the transaction/conflict keys
/// append the raw hash (and signer) bytes.
#[test]
fn storage_key_layouts_match_csharp_keybuilder() {
    let key = LedgerContract::block_hash_storage_key(0x0102_0304);
    assert_eq!(key.id(), -4);
    assert_eq!(key.key(), &[9u8, 0x01, 0x02, 0x03, 0x04]);
    // Low indices land in the high-order byte positions.
    assert_eq!(
        LedgerContract::block_hash_storage_key(7).key(),
        &[9u8, 0, 0, 0, 7]
    );

    let hash = UInt256::from_bytes(&[0xAB; 32]).unwrap();
    let mut expected = vec![11u8];
    expected.extend_from_slice(&[0xAB; 32]);
    assert_eq!(
        LedgerContract::transaction_storage_key(&hash).key(),
        &expected[..]
    );

    let signer = UInt160::from_bytes(&[0x11; 20]).unwrap();
    expected.extend_from_slice(&[0x11; 20]);
    assert_eq!(
        LedgerContract::conflict_signer_storage_key(&hash, &signer).key(),
        &expected[..]
    );

    assert_eq!(LedgerContract::current_block_storage_key().key(), &[12u8]);
    assert_eq!(LedgerContract::block_storage_key(&hash).key()[0], 5u8);
}

#[test]
fn storage_key_helpers_use_shared_builders() {
    let helpers = include_str!("../../ledger_contract/storage.rs");

    assert!(helpers.contains("prefixed_key("));
    assert!(helpers.contains("prefixed_u32_be_key("));
    assert!(helpers.contains("prefixed_hash256_key("));
    assert!(helpers.contains("prefixed_hash256_hash160_key("));
    assert!(!helpers.contains("StorageKey::create("));
    assert!(!helpers.contains("StorageKey::create_with_uint32("));
    assert!(!helpers.contains("StorageKey::create_with_uint256("));
    assert!(!helpers.contains("StorageKey::create_with_uint256_uint160("));
}

/// Byte-level pins of the C# `BinarySerializer` value layouts
/// (StackItemType: Struct = 0x41, ByteString = 0x28, Integer =
/// 0x21; integers are minimal signed little-endian var-bytes with
/// zero encoded as the empty span — Neo.VM `Integer`).
#[test]
fn value_layouts_match_csharp_binary_serializer() {
    // HashIndexState (Prefix_CurrentBlock value):
    // Struct{ ByteString(hash), Integer(index) }.
    let hash = UInt256::from_bytes(&[7u8; 32]).unwrap();
    let mut expected = vec![0x41, 0x02, 0x28, 0x20];
    expected.extend_from_slice(&[7u8; 32]);
    expected.extend_from_slice(&[0x21, 0x02, 0xD2, 0x04]); // 1234 LE
    assert_eq!(
        LedgerContract::new()
            .serialize_hash_index_state(&hash, 1234)
            .unwrap(),
        expected
    );

    // Index zero serializes as an empty Integer span.
    let mut expected = vec![0x41, 0x02, 0x28, 0x20];
    expected.extend_from_slice(&[7u8; 32]);
    expected.extend_from_slice(&[0x21, 0x00]);
    assert_eq!(
        LedgerContract::new()
            .serialize_hash_index_state(&hash, 0)
            .unwrap(),
        expected
    );

    // Conflict stub: Struct{ Integer(BlockIndex) }.
    assert_eq!(
        LedgerContract::new().serialize_conflict_stub(3).unwrap(),
        vec![0x41, 0x01, 0x21, 0x01, 0x03]
    );
    assert_eq!(
        LedgerContract::new().serialize_conflict_stub(0).unwrap(),
        vec![0x41, 0x01, 0x21, 0x00]
    );

    // Full transaction record:
    // Struct{ Integer(BlockIndex), ByteString(tx.ToArray()), Integer((byte)State) }.
    let mut tx = Transaction::new();
    tx.set_nonce(99);
    tx.set_script(vec![0x40]); // RET
    tx.set_signers(vec![neo_payloads::Signer::new(
        UInt160::from_bytes(&[0x22; 20]).unwrap(),
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let mut writer = BinaryWriter::new();
    tx.serialize(&mut writer).unwrap();
    let tx_bytes = writer.into_bytes();
    assert!(tx_bytes.len() < 0xFD, "single-byte var-int length expected");

    let record = LedgerContract::new()
        .serialize_persisted_transaction_state(7, VMState::HALT, &tx)
        .unwrap();
    let mut expected = vec![0x41, 0x03, 0x21, 0x01, 0x07, 0x28, tx_bytes.len() as u8];
    expected.extend_from_slice(&tx_bytes);
    expected.extend_from_slice(&[0x21, 0x01, 0x01]); // HALT = 1
    assert_eq!(record, expected);

    // VMState::NONE (0) is the empty Integer span.
    let record = LedgerContract::new()
        .serialize_persisted_transaction_state(7, VMState::NONE, &tx)
        .unwrap();
    let mut expected = vec![0x41, 0x03, 0x21, 0x01, 0x07, 0x28, tx_bytes.len() as u8];
    expected.extend_from_slice(&tx_bytes);
    expected.extend_from_slice(&[0x21, 0x00]);
    assert_eq!(record, expected);

    // And the reader decodes the pinned layout back.
    let state = LedgerContract::decode_transaction_state(&record).unwrap();
    assert_eq!(state.block_index, 7);
    assert_eq!(state.state, VMState::NONE);
    let decoded_tx = state.transaction.expect("full record");
    assert_eq!(decoded_tx.nonce(), 99);
}

#[test]
fn ledger_public_return_encoders_use_stack_value_projection() {
    use neo_payloads::{Header, TrimmedBlock};

    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let mut tx = Transaction::new();
    tx.set_nonce(99);
    tx.set_script(vec![0x40]);
    tx.set_signers(vec![neo_payloads::Signer::new(
        UInt160::from_bytes(&[0x22; 20]).unwrap(),
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);

    let expected_tx = BinarySerializer::serialize(
        &StackItem::try_from(tx.to_stack_value().unwrap()).unwrap(),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    assert_eq!(
        LedgerContract::transaction_to_bytes(&tx, "test").unwrap(),
        expected_tx
    );

    let legacy_signers = StackItem::from_array(
        tx.signers()
            .iter()
            .map(|signer| StackItem::try_from(signer.to_stack_value()).unwrap())
            .collect::<Vec<_>>(),
    );
    let expected_signers =
        BinarySerializer::serialize(&legacy_signers, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(
        LedgerContract::signers_to_bytes(tx.signers(), "test").unwrap(),
        expected_signers
    );

    let mut header = Header::new();
    header.set_index(77);
    header.set_nonce(u64::MAX);
    let block = TrimmedBlock::new(
        header,
        vec![
            UInt256::from_bytes(&[0x11u8; 32]).unwrap(),
            UInt256::from_bytes(&[0x22u8; 32]).unwrap(),
        ],
    );
    let expected_block = BinarySerializer::serialize(
        &StackItem::try_from(block.to_stack_value()).unwrap(),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    assert_eq!(
        LedgerContract::trimmed_block_to_bytes(&block, "test").unwrap(),
        expected_block
    );

    let source = include_str!("../../ledger_contract/wire.rs");
    let tx_helper = slice_between(source, "fn transaction_to_bytes", "fn signers_to_bytes");
    assert!(tx_helper.contains("to_stack_value"));
    assert!(tx_helper.contains("serialize_stack_value_default"));
    assert!(!tx_helper.contains("to_stack_item"));
    assert!(!tx_helper.contains("BinarySerializer::serialize("));

    let signers_helper = slice_between(source, "fn signers_to_bytes", "fn trimmed_block_to_bytes");
    assert!(signers_helper.contains("StackValue::Array"));
    assert!(signers_helper.contains("to_stack_value"));
    assert!(signers_helper.contains("serialize_stack_value_default"));
    assert!(!signers_helper.contains("StackItem::from_array"));
    assert!(!signers_helper.contains("BinarySerializer::serialize("));

    let block_helper = slice_between(
        source,
        "fn trimmed_block_to_bytes",
        "pub(crate) fn deserialize_hash_index_state",
    );
    assert!(block_helper.contains("to_stack_value"));
    assert!(block_helper.contains("serialize_stack_value_default"));
    assert!(!block_helper.contains("to_stack_item"));
    assert!(!block_helper.contains("BinarySerializer::serialize("));
}

#[test]
fn decode_transaction_state_rejects_malformed_full_record() {
    let record = BinarySerializer::serialize_stack_value_default(&StackValue::Struct(vec![
        StackValue::Integer(7),
        StackValue::ByteString(vec![0xff]),
        StackValue::Integer(VMState::HALT.to_byte() as i64),
    ]))
    .unwrap();

    let error = LedgerContract::decode_transaction_state(&record).unwrap_err();
    assert!(
        error.to_string().contains("TransactionState transaction"),
        "{error}"
    );
}

#[test]
fn hash_index_state_interoperable_projection_matches_csharp_shape() {
    let hash = UInt256::from_bytes(&[0x77; 32]).unwrap();
    let state = HashIndexState::new(hash, 1234);
    let expected_value = StackValue::Struct(vec![
        StackValue::ByteString(hash.to_bytes()),
        StackValue::Integer(1234),
    ]);

    let projected = state.to_stack_value();
    assert!(
        stack_value_struct_eq(&projected, &expected_value),
        "structural StackValue mismatch: {projected:?} vs {expected_value:?}"
    );

    let trait_value = Interoperable::to_stack_value(&state).unwrap();
    assert!(
        stack_value_struct_eq(&trait_value, &expected_value),
        "structural StackValue mismatch: {trait_value:?} vs {expected_value:?}"
    );

    let mut parsed = HashIndexState::new(UInt256::default(), 0);
    Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
    assert_eq!(parsed, state);

    assert!(HashIndexState::from_stack_value(StackValue::Array(vec![])).is_err());
    assert!(
        HashIndexState::from_stack_value(StackValue::Struct(vec![
            StackValue::ByteString(vec![0x77; 31]),
            StackValue::Integer(1)
        ]))
        .is_err()
    );
}

#[test]
fn ledger_storage_codecs_use_stack_value_projection() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let source = include_str!("../../ledger_contract/wire.rs");
    let hash_serializer = slice_between(
        source,
        "pub fn serialize_hash_index_state",
        "pub fn serialize_persisted_transaction_state",
    );
    assert!(hash_serializer.contains("HashIndexState::new"));
    assert!(hash_serializer.contains("encode_storage_struct"));
    assert!(!hash_serializer.contains("StackValue::Struct"));
    assert!(!hash_serializer.contains("StackItem::from_struct"));
    assert!(!hash_serializer.contains("BinarySerializer::serialize("));

    let tx_serializer = slice_between(
        source,
        "pub fn serialize_persisted_transaction_state",
        "pub fn serialize_conflict_stub",
    );
    assert!(tx_serializer.contains("to_stack_value"));
    assert!(tx_serializer.contains("serialize_stack_value_default"));
    assert!(!tx_serializer.contains("StackItem::from_struct"));
    assert!(!tx_serializer.contains("BinarySerializer::serialize("));

    let stub_serializer = slice_between(
        source,
        "pub fn serialize_conflict_stub",
        "pub(crate) fn transaction_to_bytes",
    );
    assert!(stub_serializer.contains("to_stack_value"));
    assert!(stub_serializer.contains("serialize_stack_value_default"));
    assert!(!stub_serializer.contains("StackItem::from_struct"));
    assert!(!stub_serializer.contains("BinarySerializer::serialize("));

    let hash_deserializer = slice_between(
        source,
        "pub(crate) fn deserialize_hash_index_state",
        "fn decode_transaction_state",
    );
    assert!(hash_deserializer.contains("decode_stack_value"));
    assert!(hash_deserializer.contains("HashIndexState::from_stack_value"));
    assert!(!hash_deserializer.contains("bytes_to_hash256"));
    assert!(!hash_deserializer.contains("stack_value_as_u32"));
    assert!(!hash_deserializer.contains("BinarySerializer::deserialize("));

    let tx_deserializer = slice_between(source, "fn decode_transaction_state", "\n}\n");
    assert!(tx_deserializer.contains("decode_stack_value"));
    assert!(tx_deserializer.contains("from_stack_value"));
    assert!(!tx_deserializer.contains("Transaction::deserialize"));
    assert!(!tx_deserializer.contains("MemoryReader::new"));
    assert!(!tx_deserializer.contains("BinarySerializer::deserialize("));
}

/// C# `LedgerContract.ContainsConflictHash`: the bare stub must
/// exist, be a stub, and be traceable; then some signer stub must
/// exist and be traceable.
#[test]
fn contains_conflict_hash_matches_csharp_rules() {
    let cache = DataCache::new(false);
    let ledger = LedgerContract::new();
    let hash = UInt256::from_bytes(&[0xCD; 32]).unwrap();
    let signer = UInt160::from_bytes(&[0x44; 20]).unwrap();
    let other = UInt160::from_bytes(&[0x55; 20]).unwrap();
    let mtb = 10u32;

    // Chain height 100 → traceable window is (90, 100].
    cache.add(
        LedgerContract::current_block_storage_key(),
        StorageItem::from_bytes(
            LedgerContract::new()
                .serialize_hash_index_state(&UInt256::from_bytes(&[1u8; 32]).unwrap(), 100)
                .unwrap(),
        ),
    );

    // No record at all → false.
    assert!(
        !ledger
            .contains_conflict_hash(&cache, &hash, &[signer], mtb)
            .unwrap()
    );

    // Bare stub (traceable) but no signer record → false.
    cache.add(
        LedgerContract::transaction_storage_key(&hash),
        StorageItem::from_bytes(LedgerContract::new().serialize_conflict_stub(95).unwrap()),
    );
    assert!(
        !ledger
            .contains_conflict_hash(&cache, &hash, &[signer], mtb)
            .unwrap()
    );

    // Signer record for a different account → still false for ours…
    cache.add(
        LedgerContract::conflict_signer_storage_key(&hash, &other),
        StorageItem::from_bytes(LedgerContract::new().serialize_conflict_stub(95).unwrap()),
    );
    assert!(
        !ledger
            .contains_conflict_hash(&cache, &hash, &[signer], mtb)
            .unwrap()
    );
    // …and true for the matching one.
    assert!(
        ledger
            .contains_conflict_hash(&cache, &hash, &[other], mtb)
            .unwrap()
    );

    // An untraceable signer record (95 - window) does not count.
    cache.add(
        LedgerContract::conflict_signer_storage_key(&hash, &signer),
        StorageItem::from_bytes(LedgerContract::new().serialize_conflict_stub(80).unwrap()),
    );
    assert!(
        !ledger
            .contains_conflict_hash(&cache, &hash, &[signer], mtb)
            .unwrap()
    );

    // A full transaction record under the hash is NOT a conflict
    // record (C#: `stub.Transaction is not null` → false).
    let mut tx = Transaction::new();
    tx.set_script(vec![0x40]);
    tx.set_signers(vec![neo_payloads::Signer::new(
        other,
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    cache.update(
        LedgerContract::transaction_storage_key(&hash),
        StorageItem::from_bytes(
            LedgerContract::new()
                .serialize_persisted_transaction_state(95, VMState::HALT, &tx)
                .unwrap(),
        ),
    );
    assert!(
        !ledger
            .contains_conflict_hash(&cache, &hash, &[other], mtb)
            .unwrap()
    );
}

#[test]
fn current_index_and_hash_round_trip_through_storage() {
    let cache = DataCache::new(false);
    let ledger = LedgerContract::new();

    // Empty ledger: C# indexes Prefix_CurrentBlock directly and faults when
    // the current-block pointer is absent.
    assert_eq!(ledger.optional_current_tip(&cache).unwrap(), None);
    assert!(ledger.current_index(&cache).is_err());
    assert!(ledger.current_hash(&cache).is_err());

    // Write a HashIndexState under the current-block key (prefix 12) and
    // read it back, exercising the exact on-disk format the engine uses.
    let hash = UInt256::from_bytes(&[7u8; 32]).unwrap();
    let bytes = LedgerContract::new()
        .serialize_hash_index_state(&hash, 1234)
        .unwrap();
    cache.add(
        LedgerContract::current_block_storage_key(),
        StorageItem::from_bytes(bytes),
    );
    assert_eq!(
        ledger.optional_current_tip(&cache).unwrap(),
        Some((hash, 1234))
    );
    assert_eq!(ledger.current_index(&cache).unwrap(), 1234);
    assert_eq!(ledger.current_hash(&cache).unwrap(), hash);
}
