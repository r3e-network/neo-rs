use super::super::request::OracleIdList;
use super::super::*;
use neo_crypto::Crypto;
use neo_primitives::{CallFlags, ContractParameterType, UInt256};
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::Interoperable;
use neo_vm_rs::StackValue;
// ===== from oracle_native_tests.rs =====
#[test]
fn native_contract_surface() {
    let c = OracleContract::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        ["getPrice", "setPrice", "request", "finish", "verify"]
    );

    let setter = c.methods().iter().find(|m| m.name == "setPrice").unwrap();
    assert!(!setter.safe);
    assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
    assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
    assert_eq!(setter.return_type, ContractParameterType::Void);

    let request = c.methods().iter().find(|m| m.name == "request").unwrap();
    assert!(!request.safe);
    assert_eq!(request.cpu_fee, 0);
    assert_eq!(
        request.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert_eq!(
        request.parameters,
        vec![
            ContractParameterType::String,
            ContractParameterType::String,
            ContractParameterType::String,
            ContractParameterType::Any,
            ContractParameterType::Integer,
        ]
    );
    assert_eq!(request.return_type, ContractParameterType::Void);

    let finish = c.methods().iter().find(|m| m.name == "finish").unwrap();
    assert!(!finish.safe);
    assert_eq!(finish.cpu_fee, 0);
    assert_eq!(
        finish.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert!(finish.parameters.is_empty());
    assert_eq!(finish.return_type, ContractParameterType::Void);

    let verify = c.methods().iter().find(|m| m.name == "verify").unwrap();
    // C# ContractMethodMetadata: Safe = (RequiredCallFlags & ~ReadOnly) == 0,
    // and Verify declares CallFlags.None.
    assert!(verify.safe);
    assert_eq!(verify.cpu_fee, 1 << 15);
    assert_eq!(verify.required_call_flags, CallFlags::NONE.bits());
    assert!(verify.parameters.is_empty());
    assert_eq!(verify.return_type, ContractParameterType::Boolean);
}

#[test]
fn set_price_write_round_trips() {
    let cache = DataCache::new(false);
    cache.add(
        OracleContract::price_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            DEFAULT_ORACLE_PRICE,
        ))),
    );
    // The setter's storage effect (overwrite Prefix_Price) is observed by
    // the getter's reader.
    OracleContract::new()
        .put_price(&cache, 7_5000000)
        .expect("initialized price can be overwritten"); // 0.75 GAS
    assert_eq!(OracleContract::new().read_price(&cache).unwrap(), 7_5000000);
}

#[test]
fn price_requires_initialized_storage() {
    let cache = DataCache::new(false);
    assert!(OracleContract::new().read_price(&cache).is_err());
    assert!(OracleContract::new().put_price(&cache, 12345678).is_err());

    let key = OracleContract::price_key();
    cache.add(
        key,
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(12345678))),
    );
    assert_eq!(OracleContract::new().read_price(&cache).unwrap(), 12345678);
}

fn sample_request(filter: Option<String>) -> OracleRequest {
    OracleRequest::new(
        UInt256::from_bytes(&[0xAA; 32]).unwrap(),
        1_0000000,
        "https://example.org/data",
        filter,
        UInt160::from_bytes(&[0xCB; 20]).unwrap(),
        "oracleCallback",
        BinarySerializer::serialize(
            &StackItem::from_int(BigInt::from(42)),
            &ExecutionEngineLimits::default(),
        )
        .unwrap(),
    )
}

#[test]
fn request_record_round_trips() {
    for filter in [Some("$.value".to_string()), None] {
        let request = sample_request(filter);
        let bytes = OracleContract::encode_oracle_request(&request).unwrap();
        let decoded = OracleContract::decode_oracle_request(&bytes).unwrap();
        assert_eq!(decoded, request);
    }
}

#[test]
fn request_record_add_rejects_duplicate_id_like_csharp() {
    // C# Request writes the record via SnapshotCache.Add, so a reused
    // request id faults and must not overwrite the existing request.
    let cache = DataCache::new(false);
    let contract = OracleContract::new();
    let original = sample_request(None);
    contract
        .add_request_record(&cache, 5, &original)
        .expect("first add succeeds");
    let replacement = sample_request(Some("$.replacement".to_string()));
    let err = contract
        .add_request_record(&cache, 5, &replacement)
        .expect_err("duplicate request id must fault");
    assert!(err.to_string().contains("duplicate request id 5"), "{err}");
    assert_eq!(contract.read_request(&cache, 5).unwrap(), Some(original));
}

#[test]
fn request_record_layout_matches_csharp_to_stack_item() {
    // C# OracleRequest.ToStackItem: an Array of 7 items —
    // [txid bytes, Integer, url, filter|Null, contract bytes, method, userdata].
    let request = sample_request(Some("$.x".to_string()));
    let bytes = OracleContract::encode_oracle_request(&request).unwrap();
    let expected_item = StackItem::from_array(vec![
        StackItem::from_byte_string(request.original_tx_id.to_bytes()),
        StackItem::from_int(BigInt::from(request.gas_for_response)),
        StackItem::from_byte_string(request.url.as_bytes().to_vec()),
        StackItem::from_byte_string(b"$.x".to_vec()),
        StackItem::from_byte_string(request.callback_contract.to_bytes()),
        StackItem::from_byte_string(request.callback_method.as_bytes().to_vec()),
        StackItem::from_byte_string(request.user_data.clone()),
    ]);
    let expected =
        BinarySerializer::serialize(&expected_item, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(
        Interoperable::to_stack_value(&request).unwrap(),
        StackValue::try_from(expected_item.clone()).unwrap()
    );

    let mut trait_decoded = OracleRequest::default();
    Interoperable::from_stack_value(
        &mut trait_decoded,
        StackValue::try_from(expected_item).unwrap(),
    )
    .unwrap();
    assert_eq!(trait_decoded, request);

    let item =
        BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();
    let StackItem::Array(array) = item else {
        panic!("OracleRequest must serialize as an Array (not Struct)");
    };
    let items = array.items();
    assert_eq!(items.len(), 7);
    assert_eq!(items[0].as_bytes().unwrap(), vec![0xAA; 32]);
    assert_eq!(items[1].as_int().unwrap(), BigInt::from(1_0000000));
    assert_eq!(
        items[2].as_bytes().unwrap(),
        b"https://example.org/data".to_vec()
    );
    assert_eq!(items[3].as_bytes().unwrap(), b"$.x".to_vec());
    assert_eq!(items[4].as_bytes().unwrap(), vec![0xCB; 20]);
    assert_eq!(items[5].as_bytes().unwrap(), b"oracleCallback".to_vec());
    assert_eq!(
        items[6].as_bytes().unwrap(),
        BinarySerializer::serialize(
            &StackItem::from_int(BigInt::from(42)),
            &ExecutionEngineLimits::default()
        )
        .unwrap()
    );

    // A null filter serializes as StackItem::Null in slot 3.
    let no_filter = OracleContract::encode_oracle_request(&sample_request(None)).unwrap();
    let StackItem::Array(array) =
        BinarySerializer::deserialize(&no_filter, &ExecutionEngineLimits::default(), None).unwrap()
    else {
        panic!("array expected");
    };
    assert!(matches!(array.items()[3], StackItem::Null));
}

#[test]
fn oracle_storage_codecs_use_stack_value_projection() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let storage_source = include_str!("../../oracle_contract/storage.rs");
    let request_encoder = slice_between(
        storage_source,
        "fn encode_oracle_request",
        "fn decode_oracle_request",
    );
    assert!(request_encoder.contains("to_stack_value"));
    assert!(request_encoder.contains("serialize_stack_value_default"));
    assert!(!request_encoder.contains("StackItem::from_array"));
    assert!(!request_encoder.contains("BinarySerializer::serialize("));

    let request_decoder = slice_between(
        storage_source,
        "fn decode_oracle_request",
        "fn encode_id_list",
    );
    assert!(request_decoder.contains("deserialize_stack_value_with_limits"));
    assert!(request_decoder.contains("OracleRequest::from_stack_value"));
    assert!(!request_decoder.contains("BinarySerializer::deserialize("));

    let id_list_encoder = slice_between(storage_source, "fn encode_id_list", "fn decode_id_list");
    assert!(id_list_encoder.contains("OracleIdList::new"));
    assert!(id_list_encoder.contains("to_stack_value"));
    assert!(!id_list_encoder.contains("StackValue::Array"));
    assert!(id_list_encoder.contains("serialize_stack_value_default"));
    assert!(!id_list_encoder.contains("StackItem::from_array"));
    assert!(!id_list_encoder.contains("BinarySerializer::serialize("));

    let id_list_decoder = slice_between(storage_source, "fn decode_id_list", "fn read_request");
    assert!(id_list_decoder.contains("deserialize_stack_value_with_limits"));
    assert!(id_list_decoder.contains("OracleIdList::from_stack_value"));
    assert!(!id_list_decoder.contains("StackValue::Array"));
    assert!(!id_list_decoder.contains("stack_value_as_bigint"));
    assert!(!id_list_decoder.contains("BinarySerializer::deserialize("));

    let source = include_str!("../../oracle_contract/mod.rs");
    // C# OracleContract.Request stores `BinarySerializer.Serialize(userData,
    // MaxUserDataLength, engine.Limits.MaxStackSize)` (OracleContract.cs:265).
    // The Rust request path only needs a value projection before reserializing
    // under that byte cap; the later `finish` callback still materializes a
    // StackItem because it queues a VM call.
    let request_user_data = slice_between(
        source,
        "// C#: UserData = BinarySerializer.Serialize(userData,",
        "let request = OracleRequest",
    );
    assert!(request_user_data.contains("deserialize_stack_value_with_limits"));
    assert!(request_user_data.contains("serialize_stack_value_with_limits"));
    assert!(!request_user_data.contains("BinarySerializer::deserialize("));
    assert!(!request_user_data.contains("BinarySerializer::serialize_with_limits("));
}

#[test]
fn invoke_request_args_use_shared_raw_parsers() {
    fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("start marker exists");
        let end_index = source[start_index..]
            .find(end)
            .map(|offset| start_index + offset)
            .expect("end marker exists");
        &source[start_index..end_index]
    }

    let source = include_str!("../../oracle_contract/mod.rs");
    let set_price = slice_between(source, "\"setPrice\" =>", "\"request\" =>");
    assert!(set_price.contains("crate::args::raw_i64_arg"));
    assert!(!set_price.contains("BigInt::from_signed_bytes_le(args"));
    assert!(!set_price.contains("BigInt::from_signed_bytes_le(b)"));

    let request = slice_between(source, "\"request\" =>", "\"finish\" =>");
    assert!(request.contains("crate::args::raw_string_arg"));
    assert!(request.contains("crate::args::raw_i64_arg"));
    assert!(!request.contains("String::from_utf8("));
    assert!(!request.contains("BigInt::from_signed_bytes_le(args"));
    assert!(!request.contains("BigInt::from_signed_bytes_le(b)"));
}

#[test]
fn id_list_round_trips_and_key_uses_url_hash160() {
    let ids = vec![0u64, 1, 7, u64::from(u32::MAX) + 5, u64::MAX];
    let bytes = OracleContract::encode_id_list(&ids).unwrap();
    let expected = BinarySerializer::serialize(
        &StackItem::from_array(
            ids.iter()
                .map(|id| StackItem::from_int(BigInt::from(*id)))
                .collect::<Vec<_>>(),
        ),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(OracleContract::decode_id_list(&bytes).unwrap(), ids);

    // C# GetUrlHash = Crypto.Hash160(strict utf8 url) appended to Prefix_IdList.
    let url = "https://example.org/data";
    let key = OracleContract::id_list_key(url);
    let mut expected = vec![PREFIX_ID_LIST];
    expected.extend_from_slice(&Crypto::hash160(url.as_bytes()));
    assert_eq!(key.key(), expected.as_slice());

    // Request key is Prefix_Request ++ big-endian id.
    let rkey = OracleContract::request_key(0x0102030405060708);
    assert_eq!(
        rkey.key(),
        [
            PREFIX_REQUEST,
            0x01,
            0x02,
            0x03,
            0x04,
            0x05,
            0x06,
            0x07,
            0x08
        ]
    );
}

#[test]
fn oracle_id_list_interoperable_projection_matches_csharp_shape() {
    let ids = vec![0u64, 7, u64::MAX];
    let state = OracleIdList::new(ids.clone());
    let expected_value = StackValue::Array(
        ids.iter()
            .map(|id| StackValue::BigInteger(BigInt::from(*id).to_signed_bytes_le()))
            .collect::<Vec<_>>(),
    );

    let trait_value = Interoperable::to_stack_value(&state).unwrap();
    assert_eq!(trait_value, expected_value);

    let mut parsed = OracleIdList::new(Vec::new());
    Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
    assert_eq!(parsed.into_ids(), ids);
}

#[test]
fn request_id_counter_round_trips() {
    let cache = DataCache::new(false);
    // C# genesis initialization seeds Prefix_RequestId. Later reads use
    // GetAndChange(...), so a missing counter faults instead of inventing 0.
    assert!(OracleContract::new().read_request_id(&cache).is_err());
    OracleContract::new().write_request_id(&cache, &BigInt::from(1));
    assert_eq!(OracleContract::new().read_request_id(&cache).unwrap(), 1);
    OracleContract::new().write_request_id(&cache, &BigInt::from(u64::MAX));
    assert_eq!(
        OracleContract::new().read_request_id(&cache).unwrap(),
        u64::MAX
    );
}

#[test]
fn request_queries_resolve_storage() {
    let cache = DataCache::new(false);
    let contract = OracleContract::new();
    assert!(contract.get_request(&cache, 1).unwrap().is_none());
    assert!(contract.get_requests(&cache).is_empty());
    assert!(
        contract
            .get_requests_by_url(&cache, "https://example.org/data")
            .unwrap()
            .is_empty()
    );

    let request = sample_request(None);
    cache.add(
        OracleContract::request_key(3),
        StorageItem::from_bytes(OracleContract::encode_oracle_request(&request).unwrap()),
    );
    cache.add(
        OracleContract::id_list_key(&request.url),
        StorageItem::from_bytes(OracleContract::encode_id_list(&[3]).unwrap()),
    );

    assert_eq!(
        contract.get_request(&cache, 3).unwrap(),
        Some(request.clone())
    );
    assert_eq!(contract.get_requests(&cache), vec![(3, request.clone())]);
    assert_eq!(
        contract.get_requests_by_url(&cache, &request.url).unwrap(),
        vec![(3, request.clone())]
    );

    // The native-contract seam exposes the same record to the engine.
    let details = NativeContract::oracle_request_url_full(&contract, &cache, 3)
        .unwrap()
        .expect("details");
    assert_eq!(details.url, request.url);
    assert_eq!(details.original_tx_id, request.original_tx_id);
}

/// C# `OracleContract.OnManifestCompose` (OracleContract.cs:58-64): no
/// standards before HF_Faun, NEP-30 from the Faun height — and the Faun
/// boundary is a manifest-refresh activation (`Activations => [null,
/// HF_Faun]`, OracleContract.cs:56).
#[test]
fn manifest_standards_gain_nep30_at_faun() {
    use neo_config::{Hardfork, ProtocolSettings};
    use neo_execution::native_contract::build_native_contract_state;

    let unscheduled = build_native_contract_state(&OracleContract, &ProtocolSettings::default(), 0);
    assert!(unscheduled.manifest.supported_standards.is_empty());

    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfFaun, 10);
    let before = build_native_contract_state(&OracleContract, &settings, 9);
    assert!(before.manifest.supported_standards.is_empty());
    let after = build_native_contract_state(&OracleContract, &settings, 10);
    assert_eq!(after.manifest.supported_standards, ["NEP-30"]);

    assert_eq!(
        NativeContract::activations(&OracleContract),
        &[Hardfork::HfFaun]
    );
}
