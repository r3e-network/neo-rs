use super::*;
use neo_manifest::ContractManifest;
use neo_vm::StackItem;
use neo_vm_rs::StackValue;

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
fn contract_state_roundtrip_matches_signed_id() {
    let nef = NefFile::new("compiler".to_string(), vec![1, 2, 3]);
    let manifest = ContractManifest::new("test".to_string());
    let state = ContractState::new(-1, UInt160::zero(), nef.clone(), manifest.clone());

    let mut writer = BinaryWriter::new();
    state.serialize(&mut writer).expect("serialize");
    let bytes = writer.into_bytes();

    let mut reader = MemoryReader::new(&bytes);
    let parsed = ContractState::deserialize(&mut reader).expect("deserialize");

    assert_eq!(parsed.id, state.id);
    assert_eq!(parsed.update_counter, state.update_counter);
    assert_eq!(parsed.hash, state.hash);
    assert_eq!(parsed.nef.script, nef.script);
    assert_eq!(parsed.manifest.name, manifest.name);
    assert_eq!(bytes.len(), state.size());
}

#[test]
fn contract_state_projects_to_stack_value() {
    let hash = UInt160::from_bytes(&[1u8; 20]).expect("hash");
    let nef = NefFile::new("compiler".to_string(), vec![1, 2, 3]);
    let manifest = ContractManifest::new("test".to_string());
    let mut state = ContractState::new(-7, hash, nef.clone(), manifest.clone());
    state.update_counter = 9;

    let left = state.to_stack_value();
    let right = StackValue::Array(vec![
        StackValue::Integer(-7),
        StackValue::Integer(9),
        StackValue::ByteString(hash.to_bytes()),
        StackValue::ByteString(nef.to_bytes()),
        manifest.to_stack_value(),
    ]);
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn contract_state_reads_stack_value() {
    let hash = UInt160::from_bytes(&[2u8; 20]).expect("hash");
    let nef = NefFile::new("compiler".to_string(), vec![4, 5, 6]);
    let manifest = ContractManifest::new("parsed".to_string());

    let mut state = ContractState::default();
    state
        .from_stack_value(StackValue::Array(vec![
            StackValue::Integer(11),
            StackValue::Integer(3),
            StackValue::ByteString(hash.to_bytes()),
            StackValue::ByteString(nef.to_bytes()),
            manifest.to_stack_value(),
        ]))
        .expect("contract state from stack value");

    assert_eq!(state.id, 11);
    assert_eq!(state.update_counter, 3);
    assert_eq!(state.hash, hash);
    assert_eq!(state.nef.script, nef.script);
    assert_eq!(state.manifest.name, manifest.name);
}

#[test]
fn contract_state_rejects_invalid_integer_fields() {
    let hash = UInt160::from_bytes(&[4u8; 20]).expect("hash");
    let nef = NefFile::new("compiler".to_string(), vec![1, 2, 3]);
    let manifest = ContractManifest::new("integer-bounds".to_string());

    let stack_value = |id, update_counter| {
        StackValue::Array(vec![
            id,
            update_counter,
            StackValue::ByteString(hash.to_bytes()),
            StackValue::ByteString(nef.to_bytes()),
            manifest.to_stack_value(),
        ])
    };

    let oversized_id = StackValue::BigInteger(vec![0x01; 33]);
    assert!(
        ContractState::default()
            .from_stack_value(stack_value(oversized_id, StackValue::Integer(0)))
            .is_err()
    );

    let overflowing_id = num_bigint::BigInt::from(i64::from(i32::MAX) + 1).to_signed_bytes_le();
    assert!(
        ContractState::default()
            .from_stack_value(stack_value(
                StackValue::BigInteger(overflowing_id),
                StackValue::Integer(0)
            ))
            .is_err()
    );

    assert!(
        ContractState::default()
            .from_stack_value(stack_value(StackValue::Integer(0), StackValue::Integer(-1)))
            .is_err()
    );

    assert!(
        ContractState::default()
            .from_stack_value(stack_value(
                StackValue::Integer(0),
                StackValue::Integer(i64::from(u16::MAX) + 1)
            ))
            .is_err()
    );
}

#[test]
fn contract_state_stack_item_projection_matches_stack_value_projection() {
    let hash = UInt160::from_bytes(&[3u8; 20]).expect("hash");
    let nef = NefFile::new("compiler".to_string(), vec![7, 8, 9]);
    let manifest = ContractManifest::new("adapter".to_string());
    let state = ContractState::new(4, hash, nef, manifest);
    let expected = StackItem::try_from(state.to_stack_value()).unwrap();

    let trait_sv = <ContractState as neo_vm::Interoperable>::to_stack_value(&state).unwrap();
    assert_eq!(StackItem::try_from(trait_sv).unwrap(), expected);
}

#[test]
fn contract_record_pins_the_interoperable_stack_item_encoding() {
    // The stored Prefix_Contract(8) record must be the C# interoperable
    // form: BinarySerializer.Serialize(ContractState.ToStackItem(null)),
    // i.e. an Array of [Integer(Id), Integer(UpdateCounter),
    // ByteString(Hash), ByteString(Nef.ToArray()), Manifest.ToStackItem()]
    // — verified against neo_csharp ContractState.cs / StorageItem.cs.
    let hash = UInt160::from_bytes(&[0x11u8; 20]).expect("hash");
    let nef = NefFile::new("compiler".to_string(), vec![0x40]);
    let mut manifest = ContractManifest::new("Fixture".to_string());
    manifest.supported_standards = vec!["NEP-17".to_string()];
    let mut state = ContractState::new(7, hash, nef.clone(), manifest);
    state.update_counter = 9;

    let record = state.serialize_contract_record().expect("record bytes");

    // Self-consistency: the record equals the Rust BinarySerializer run
    // over a HAND-BUILT stack tree assembled per the C# composition rules
    // (ContractState.ToStackItem + ContractManifest.ToStackItem).
    let expected_value = StackValue::Array(vec![
        StackValue::Integer(7),
        StackValue::Integer(9),
        StackValue::ByteString(hash.to_bytes()),
        StackValue::ByteString(nef.to_bytes()),
        StackValue::Struct(vec![
            StackValue::ByteString(b"Fixture".to_vec()),
            StackValue::Array(Vec::new()), // groups
            StackValue::Map(Vec::new()),   // features (always empty)
            StackValue::Array(vec![StackValue::ByteString(b"NEP-17".to_vec())]),
            StackValue::Struct(vec![
                StackValue::Array(Vec::new()), // abi.methods
                StackValue::Array(Vec::new()), // abi.events
            ]),
            // permissions: the default wildcard permission is
            // Struct[Null(contract), Null(methods)].
            StackValue::Array(vec![StackValue::Struct(vec![
                StackValue::Null,
                StackValue::Null,
            ])]),
            StackValue::Null,                         // trusts wildcard
            StackValue::ByteString(b"null".to_vec()), // extra absent
        ]),
    ]);
    let expected = neo_serialization::BinarySerializer::serialize(
        &StackItem::try_from(expected_value).expect("expected stack item"),
        &neo_vm_rs::ExecutionEngineLimits::default(),
    )
    .expect("expected bytes");
    assert_eq!(record, expected);

    // Structural pinning straight from the C# BinarySerializer wire rules
    // so a regression to the raw neo_io field encoding (which would start
    // with the little-endian Id `07 00 00 00`) cannot pass:
    //   Array(0x40) tag + var-int element count 5,
    //   Integer(0x21) tag + var-bytes minimal signed-LE payloads,
    //   ByteString(0x28) tag + var-bytes payloads,
    //   Struct(0x41) tag + var-int count 8 for the manifest.
    assert_eq!(
        record[0], 0x40,
        "record must start with the Array type byte"
    );
    assert_eq!(record[1], 5, "ContractState projects exactly 5 elements");
    assert_eq!(
        &record[2..5],
        &[0x21, 1, 7],
        "Id: Integer, signed-LE minimal"
    );
    assert_eq!(&record[5..8], &[0x21, 1, 9], "UpdateCounter: Integer");
    assert_eq!(record[8], 0x28, "Hash is a ByteString");
    assert_eq!(record[9], 20, "Hash payload is 20 bytes");
    assert_eq!(&record[10..30], hash.to_bytes().as_slice());
    let nef_bytes = nef.to_bytes();
    assert!(
        nef_bytes.len() < 0xFD,
        "fixture NEF stays in 1-byte var-int range"
    );
    assert_eq!(record[30], 0x28, "NEF is a ByteString of Nef.ToArray()");
    assert_eq!(record[31] as usize, nef_bytes.len());
    assert_eq!(&record[32..32 + nef_bytes.len()], nef_bytes.as_slice());
    let manifest_offset = 32 + nef_bytes.len();
    assert_eq!(record[manifest_offset], 0x41, "manifest is a Struct");
    assert_eq!(record[manifest_offset + 1], 8, "manifest has 8 fields");

    // And the record must NOT be the legacy raw neo_io encoding.
    let mut writer = BinaryWriter::new();
    Serializable::serialize(&state, &mut writer).expect("legacy serialize");
    assert_ne!(record, writer.into_bytes());
}

#[test]
fn contract_record_roundtrips_with_nested_manifest() {
    use neo_manifest::{
        ContractAbi, ContractEventDescriptor, ContractMethodDescriptor,
        ContractParameterDefinition, ContractPermissionDescriptor, WildCardContainer,
    };
    use neo_primitives::ContractParameterType;

    let hash = UInt160::from_bytes(&[0x22u8; 20]).expect("hash");
    let nef = NefFile::new("compiler".to_string(), vec![1, 2, 3]);
    let mut manifest = ContractManifest::new("RoundTrip".to_string());
    manifest.supported_standards = vec!["NEP-17".to_string()];
    manifest.abi = ContractAbi::new(
        vec![
            ContractMethodDescriptor::new(
                "transfer".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "from".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("param"),
                    ContractParameterDefinition::new(
                        "amount".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("param"),
                ],
                ContractParameterType::Boolean,
                3,
                false,
            )
            .expect("method"),
        ],
        vec![ContractEventDescriptor::new("Transfer".to_string(), Vec::new()).expect("event")],
    );
    manifest.trusts = WildCardContainer::create(vec![ContractPermissionDescriptor::Hash(
        UInt160::from_bytes(&[5u8; 20]).expect("trust hash"),
    )]);
    // Negative ids (native range) must survive the signed-LE Integer leg.
    let mut state = ContractState::new(-3, hash, nef, manifest);
    state.update_counter = 2;

    let record = state.serialize_contract_record().expect("record bytes");
    let parsed = ContractState::deserialize_contract_record(&record).expect("parse record");

    assert_eq!(parsed.id, -3);
    assert_eq!(parsed.update_counter, 2);
    assert_eq!(parsed.hash, state.hash);
    assert_eq!(parsed.nef.script, state.nef.script);
    assert_eq!(parsed.manifest.name, "RoundTrip");
    assert_eq!(
        parsed.manifest.supported_standards,
        vec!["NEP-17".to_string()]
    );
    assert_eq!(parsed.manifest.abi.methods.len(), 1);
    assert_eq!(parsed.manifest.abi.methods[0].name, "transfer");
    assert_eq!(parsed.manifest.abi.methods[0].parameters.len(), 2);
    assert_eq!(parsed.manifest.abi.methods[0].offset, 3);
    assert_eq!(parsed.manifest.abi.events.len(), 1);
    assert_eq!(parsed.manifest.abi.events[0].name, "Transfer");
    assert_eq!(parsed.manifest.trusts, state.manifest.trusts);

    // Re-encoding the parsed state must reproduce identical record bytes.
    assert_eq!(
        parsed.serialize_contract_record().expect("re-encode"),
        record
    );
}

#[test]
fn contract_record_rejects_top_level_struct_like_csharp() {
    // C# ContractState.FromStackItem casts the outer item to
    // Neo.VM.Types.Array. The nested manifest is a Struct, but a Struct at
    // the ContractState root must fail even if all five fields are valid.
    let hash = UInt160::from_bytes(&[0x33u8; 20]).expect("hash");
    let nef = NefFile::new("compiler".to_string(), vec![0x40]);
    let manifest = ContractManifest::new("StructRoot".to_string());
    let malformed = StackValue::Struct(vec![
        StackValue::Integer(7),
        StackValue::Integer(0),
        StackValue::ByteString(hash.to_bytes()),
        StackValue::ByteString(nef.to_bytes()),
        manifest.to_stack_value(),
    ]);

    assert!(
        ContractState::default()
            .from_stack_value(malformed.clone())
            .is_err()
    );

    let record = neo_serialization::BinarySerializer::serialize(
        &StackItem::try_from(malformed).expect("malformed stack item"),
        &neo_vm_rs::ExecutionEngineLimits::default(),
    )
    .expect("malformed record bytes");
    assert!(ContractState::deserialize_contract_record(&record).is_err());
}

#[test]
fn contract_record_rejects_legacy_raw_encoding() {
    // A legacy raw neo_io record (i32 id first) must NOT decode as an
    // interoperable record: 0x07 is not a valid stack item type tag.
    let hash = UInt160::from_bytes(&[9u8; 20]).expect("hash");
    let nef = NefFile::new("compiler".to_string(), vec![0x40]);
    let manifest = ContractManifest::new("Legacy".to_string());
    let state = ContractState::new(7, hash, nef, manifest);

    let mut writer = BinaryWriter::new();
    Serializable::serialize(&state, &mut writer).expect("legacy serialize");
    let legacy = writer.into_bytes();

    assert!(ContractState::deserialize_contract_record(&legacy).is_err());
}
