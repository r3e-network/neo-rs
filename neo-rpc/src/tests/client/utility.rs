use super::*;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use neo_payloads::OracleResponseCode;
use neo_payloads::WitnessCondition;
use neo_payloads::{Signer, TransactionAttribute};
use neo_primitives::{ADDRESS_SIZE, UInt256, WitnessScope};
// Brings the `Witness` trait's `invocation_script`/`verification_script`
// accessors into scope (the concrete type is `neo_payloads::Witness`).
use neo_primitives::Witness as _;
use neo_serialization::json::JArray;

#[test]
fn production_client_native_hashes_use_static_helpers() {
    let sources = [
        (
            "wallet API",
            include_str!("../../client/apis/wallet_api.rs"),
        ),
        (
            "policy API",
            include_str!("../../client/apis/policy_api.rs"),
        ),
        (
            "transaction manager",
            include_str!("../../client/transactions/transaction_manager.rs"),
        ),
    ];

    for (name, source) in sources {
        assert!(
            !source.contains("NeoToken::new().hash()"),
            "{name} should use the RPC client native-hash helper for NEO"
        );
        assert!(
            !source.contains("GasToken::new().hash()"),
            "{name} should use the RPC client native-hash helper for GAS"
        );
        assert!(
            !source.contains("PolicyContract::new().hash()"),
            "{name} should use the RPC client native-hash helper for Policy"
        );
    }

    let helper_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/client/native_hashes.rs");
    let helper = std::fs::read_to_string(&helper_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", helper_path.display()));
    assert!(helper.contains("NeoToken::script_hash()"));
    assert!(helper.contains("GasToken::script_hash()"));
    assert!(helper.contains("PolicyContract::script_hash()"));
}

#[test]
fn to_script_hash_accepts_address() {
    let hash = UInt160::zero();
    let address = hash.to_address();
    let token = JToken::String(address.clone());
    let parsed = RpcUtility::to_script_hash(&token, &ProtocolSettings::default_settings()).unwrap();
    assert_eq!(parsed, hash);

    let uppercase_prefixed_hash = format!("0X{}", strip_hex_prefix(&hash.to_string()));
    let token = JToken::String(uppercase_prefixed_hash);
    let parsed = RpcUtility::to_script_hash(&token, &ProtocolSettings::default_settings()).unwrap();
    assert_eq!(parsed, hash);

    assert_eq!(
        RpcUtility::get_script_hash(&hash.to_string(), &ProtocolSettings::default_settings())
            .unwrap(),
        hash
    );
}

#[test]
fn get_script_hash_accepts_public_key_hex() {
    let keypair = KeyPair::generate().expect("keypair");
    let pubkey_hex = hex::encode(keypair.compressed_public_key());
    let expected_point = ECPoint::decode_compressed_with_curve(
        ECCurve::Secp256r1,
        &hex::decode(&pubkey_hex).unwrap(),
    )
    .unwrap();
    let expected_script = Contract::create_signature_redeem_script(expected_point);
    let expected_hash = UInt160::from_script(&expected_script);

    let parsed =
        RpcUtility::get_script_hash(&pubkey_hex, &ProtocolSettings::default_settings()).unwrap();
    assert_eq!(parsed, expected_hash);
}

#[test]
fn get_key_pair_accepts_wif_and_hex() {
    let wif = "KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p";
    let expected = KeyPair::from_wif(wif).expect("keypair");

    let parsed = RpcUtility::key_pair(wif).expect("wif parse");
    assert_eq!(parsed, expected);

    let hex_key = hex::encode(expected.private_key());
    let parsed = RpcUtility::key_pair(&hex_key).expect("hex parse");
    assert_eq!(parsed, expected);

    let hex_prefixed = format!("0x{hex_key}");
    let parsed = RpcUtility::key_pair(&hex_prefixed).expect("hex parse");
    assert_eq!(parsed, expected);

    let uppercase_hex_prefixed = format!("0X{hex_key}");
    let parsed = RpcUtility::key_pair(&uppercase_hex_prefixed).expect("hex parse");
    assert_eq!(parsed, expected);
}

#[test]
fn get_key_pair_rejects_invalid_input() {
    let err = RpcUtility::key_pair("").expect_err("empty");
    assert_eq!(err.to_string(), "Key cannot be empty");

    let err = RpcUtility::key_pair("00").expect_err("invalid");
    assert_eq!(err.to_string(), "Invalid key format");
}

#[test]
fn get_script_hash_accepts_hash_and_rejects_invalid() {
    let hash = UInt160::zero();
    let hash_string = hex::encode(hash.to_array());

    let parsed =
        RpcUtility::get_script_hash(&hash_string, &ProtocolSettings::default_settings()).unwrap();
    assert_eq!(parsed, hash);

    let prefixed = format!("0x{hash_string}");
    let parsed =
        RpcUtility::get_script_hash(&prefixed, &ProtocolSettings::default_settings()).unwrap();
    assert_eq!(parsed, hash);

    let uppercase_prefixed = format!("0X{hash_string}");
    let parsed =
        RpcUtility::get_script_hash(&uppercase_prefixed, &ProtocolSettings::default_settings())
            .unwrap();
    assert_eq!(parsed, hash);

    let err =
        RpcUtility::get_script_hash("", &ProtocolSettings::default_settings()).expect_err("empty");
    assert_eq!(err.to_string(), "Account cannot be empty");

    let err = RpcUtility::get_script_hash("00", &ProtocolSettings::default_settings())
        .expect_err("invalid");
    assert_eq!(err.to_string(), "Invalid account format");
}

#[test]
fn as_script_hash_maps_native_contract_name_and_id() {
    let contract = neo_native_contracts::standard_native_contracts()
        .into_iter()
        .find(|contract| contract.name() == "NeoToken")
        .expect("NeoToken contract");
    let expected_hash = contract.hash().to_string();
    let name = contract.name().to_string();
    let id = contract.id().to_string();

    assert_eq!(RpcUtility::as_script_hash(&name), expected_hash);
    assert_eq!(RpcUtility::as_script_hash(&id), expected_hash);
}

#[test]
fn witness_roundtrip_from_json() {
    let invocation = vec![1u8; ADDRESS_SIZE];
    let verification = vec![2u8; ADDRESS_SIZE];
    let mut obj = JObject::new();
    obj.insert(
        "invocation".to_string(),
        JToken::String(BASE64.encode(&invocation)),
    );
    obj.insert(
        "verification".to_string(),
        JToken::String(BASE64.encode(&verification)),
    );

    let witness = RpcUtility::witness_from_json(&obj).expect("witness");
    assert_eq!(witness.invocation_script(), invocation);
    assert_eq!(witness.verification_script(), verification);
}

#[test]
fn stack_item_parses_boolean() {
    let mut obj = JObject::new();
    obj.insert("type".to_string(), JToken::String("Boolean".to_string()));
    obj.insert("value".to_string(), JToken::Boolean(true));

    let item = RpcUtility::stack_item_from_json(&obj).expect("stack item");
    assert!(matches!(item, StackValue::Boolean(true)));
}

#[test]
fn stack_item_parses_interop_interface_without_value() {
    let mut obj = JObject::new();
    obj.insert(
        "type".to_string(),
        JToken::String("InteropInterface".to_string()),
    );
    obj.insert("id".to_string(), JToken::String("iter-1".to_string()));

    let item = RpcUtility::stack_item_from_json(&obj).expect("stack item");
    assert!(matches!(item, StackValue::Interop(0)));
}

#[test]
fn stack_item_parses_bytestring_and_buffer() {
    let bytes = vec![1u8, 2, 3];
    let encoded = BASE64.encode(&bytes);

    let mut bytestring = JObject::new();
    bytestring.insert("type".to_string(), JToken::String("ByteString".to_string()));
    bytestring.insert("value".to_string(), JToken::String(encoded.clone()));
    let item = RpcUtility::stack_item_from_json(&bytestring).expect("bytestring");
    assert_eq!(item.as_bytes().expect("bytestring bytes"), bytes);

    let mut buffer = JObject::new();
    buffer.insert("type".to_string(), JToken::String("Buffer".to_string()));
    buffer.insert("value".to_string(), JToken::String(encoded));
    let item = RpcUtility::stack_item_from_json(&buffer).expect("buffer");
    assert_eq!(item.as_bytes().expect("buffer bytes"), bytes);
}

#[test]
fn stack_item_parses_pointer_and_any() {
    let mut pointer = JObject::new();
    pointer.insert("type".to_string(), JToken::String("Pointer".to_string()));
    pointer.insert("value".to_string(), JToken::String("7".to_string()));

    let item = RpcUtility::stack_item_from_json(&pointer).expect("pointer");
    assert!(matches!(item, StackValue::Pointer(7)));

    let mut any = JObject::new();
    any.insert("type".to_string(), JToken::String("Any".to_string()));
    let item = RpcUtility::stack_item_from_json(&any).expect("any");
    assert!(matches!(item, StackValue::Null));
}

#[test]
fn stack_item_parses_any_with_value() {
    let mut any = JObject::new();
    any.insert("type".to_string(), JToken::String("Any".to_string()));
    any.insert("value".to_string(), JToken::String("data".to_string()));
    let item = RpcUtility::stack_item_from_json(&any).expect("any");
    assert_eq!(item.as_bytes().expect("bytes"), b"data");
}

#[test]
fn stack_item_fallbacks_for_unknown_type() {
    let mut unknown = JObject::new();
    unknown.insert("type".to_string(), JToken::String("Unknown".to_string()));
    unknown.insert("value".to_string(), JToken::String("hello".to_string()));

    let item = RpcUtility::stack_item_from_json(&unknown).expect("fallback");
    assert_eq!(item.as_bytes().expect("bytes"), b"hello");

    let mut empty = JObject::new();
    empty.insert("type".to_string(), JToken::String("Unknown".to_string()));
    let item = RpcUtility::stack_item_from_json(&empty).expect("fallback null");
    assert!(matches!(item, StackValue::Null));
}

#[test]
fn transaction_roundtrip_json() {
    use neo_payloads::witness::Witness as PayloadWitness;
    use neo_primitives::WitnessScope;

    let mut tx = Transaction::new();
    tx.set_version(1);
    tx.set_nonce(42);
    tx.set_script(vec![1, 2, 3, 4]);
    tx.set_system_fee(10);
    tx.set_network_fee(5);
    tx.set_valid_until_block(100);

    let signer = Signer::new(UInt160::zero(), WitnessScope::GLOBAL);
    tx.set_signers(vec![signer.clone()]);

    let witness = PayloadWitness::new_with_scripts(vec![1, 2], vec![3, 4]);
    tx.set_witnesses(vec![witness]);

    let json = RpcUtility::transaction_to_json(&tx, &ProtocolSettings::default_settings());
    let parsed = RpcUtility::transaction_from_json(&json, &ProtocolSettings::default_settings())
        .expect("parse transaction");

    assert_eq!(parsed.version(), tx.version());
    assert_eq!(parsed.nonce(), tx.nonce());
    assert_eq!(parsed.system_fee(), tx.system_fee());
    assert_eq!(parsed.network_fee(), tx.network_fee());
    assert_eq!(parsed.valid_until_block(), tx.valid_until_block());
    assert_eq!(parsed.script(), tx.script());
    assert_eq!(parsed.signers().len(), 1);
    assert_eq!(parsed.signers()[0].account, signer.account);
    assert_eq!(parsed.witnesses().len(), 1);
}

#[test]
fn transaction_from_json_rejects_empty_optional_array_entries() {
    for (field, expected) in [
        ("signers", "Signer entry must be an object"),
        ("attributes", "Transaction attribute must be an object"),
        ("witnesses", "Witness entry must be an object"),
    ] {
        let mut entries = JArray::new();
        entries.add(None);
        let mut json = JObject::new();
        json.insert(field.to_string(), JToken::Array(entries));

        let err = RpcUtility::transaction_from_json(&json, &ProtocolSettings::default_settings())
            .expect_err("empty array slot should fail");
        assert_eq!(err.to_string(), expected);
    }
}

#[test]
fn block_roundtrip_json() {
    let witness = Witness::new_with_scripts(vec![0xAA, 0xBB], vec![0xCC, 0xDD]);
    let header = BlockHeader::new_with_witnesses(
        0,
        UInt256::zero(),
        UInt256::zero(),
        12345,
        999,
        7,
        2,
        UInt160::zero(),
        vec![witness.clone()],
    );
    let mut tx = Transaction::new();
    tx.set_script(vec![9, 9, 9]);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
    let block = Block::from_parts(header, vec![tx]);

    let json = RpcUtility::block_to_json(&block, &ProtocolSettings::default_settings());
    let parsed = RpcUtility::block_from_json(&json, &ProtocolSettings::default_settings()).unwrap();

    assert_eq!(parsed.header.index(), block.header.index());
    assert_eq!(parsed.header.timestamp(), block.header.timestamp());
    assert_eq!(parsed.transactions.len(), 1);
    assert_eq!(parsed.transactions[0].script(), b"\t\t\t");
    // header carries exactly one witness by type (single `witness` field)
    assert_eq!(
        parsed.header.witness.invocation_script(),
        witness.invocation_script()
    );
}

#[test]
fn header_from_json_rejects_empty_witness_entry() {
    let mut witnesses = JArray::new();
    witnesses.add(None);

    let mut json = JObject::new();
    json.insert("version".to_string(), JToken::Number(0.0));
    json.insert(
        "previousblockhash".to_string(),
        JToken::String(UInt256::zero().to_string()),
    );
    json.insert(
        "merkleroot".to_string(),
        JToken::String(UInt256::zero().to_string()),
    );
    json.insert("time".to_string(), JToken::Number(123.0));
    json.insert(
        "nonce".to_string(),
        JToken::String(format!("{:016X}", 42u64)),
    );
    json.insert("index".to_string(), JToken::Number(5.0));
    json.insert("primary".to_string(), JToken::Number(0.0));
    json.insert(
        "nextconsensus".to_string(),
        JToken::String(UInt160::zero().to_string()),
    );
    json.insert("witnesses".to_string(), JToken::Array(witnesses));

    let err = RpcUtility::header_from_json(&json, &ProtocolSettings::default_settings())
        .expect_err("empty witness slot should fail");
    assert_eq!(err.to_string(), "Witness entry must be an object");
}

#[test]
fn transaction_roundtrip_with_custom_signer() {
    use neo_payloads::WitnessRuleAction;

    let mut tx = Transaction::new();
    tx.set_nonce(999);
    tx.set_script(vec![7, 7, 7]);

    let mut signer = Signer::new(UInt160::zero(), WitnessScope::CUSTOM_CONTRACTS);
    signer
        .allowed_contracts
        .push(UInt160::parse("0102030405060708090a0b0c0d0e0f1011121314").unwrap());
    signer.scopes |= WitnessScope::CUSTOM_GROUPS | WitnessScope::WITNESS_RULES;

    let key = KeyPair::generate().unwrap();
    let group_point = ECPoint::from_bytes(&key.compressed_public_key()).unwrap();
    signer.allowed_groups.push(group_point);
    signer.rules.push(neo_payloads::WitnessRule::new(
        WitnessRuleAction::Deny,
        WitnessCondition::CalledByEntry,
    ));

    tx.set_signers(vec![signer.clone()]);

    let json = RpcUtility::transaction_to_json(&tx, &ProtocolSettings::default_settings());
    let parsed = RpcUtility::transaction_from_json(&json, &ProtocolSettings::default_settings())
        .expect("parse transaction");

    assert_eq!(parsed.signers().len(), 1);
    let parsed_signer = &parsed.signers()[0];
    assert_eq!(parsed_signer.account, signer.account);
    assert!(
        parsed_signer
            .scopes
            .contains(WitnessScope::CUSTOM_CONTRACTS)
    );
    assert_eq!(parsed_signer.allowed_contracts, signer.allowed_contracts);
    assert_eq!(parsed_signer.allowed_groups.len(), 1);
    assert_eq!(parsed_signer.rules.len(), 1);
}

#[test]
fn transaction_roundtrip_preserves_attributes() {
    use neo_payloads::OracleResponseCode;
    use neo_payloads::conflicts::Conflicts;
    use neo_payloads::not_valid_before::NotValidBefore;
    use neo_payloads::notary_assisted::NotaryAssisted;
    use neo_payloads::oracle_response::OracleResponse;

    let mut tx = Transaction::new();
    tx.set_nonce(1);
    tx.set_script(vec![1]);

    let attributes = vec![
        TransactionAttribute::HighPriority,
        TransactionAttribute::NotValidBefore(NotValidBefore::new(42)),
        TransactionAttribute::Conflicts(Conflicts::new(UInt256::zero())),
        TransactionAttribute::NotaryAssisted(NotaryAssisted::new(3)),
        TransactionAttribute::OracleResponse(OracleResponse::new(
            7,
            OracleResponseCode::Timeout,
            b"result".to_vec(),
        )),
    ];
    tx.set_attributes(attributes.clone());

    let json = RpcUtility::transaction_to_json(&tx, &ProtocolSettings::default_settings());
    let parsed = RpcUtility::transaction_from_json(&json, &ProtocolSettings::default_settings())
        .expect("parse transaction");

    assert_eq!(parsed.attributes().len(), attributes.len());
    // Check representative attributes for correctness
    assert!(matches!(
        parsed.attributes()[0],
        TransactionAttribute::HighPriority
    ));
    if let TransactionAttribute::NotValidBefore(nvb) = &parsed.attributes()[1] {
        assert_eq!(nvb.height, 42);
    } else {
        panic!("expected NotValidBefore");
    }
    if let TransactionAttribute::Conflicts(conflicts) = &parsed.attributes()[2] {
        assert_eq!(conflicts.hash, UInt256::zero());
    } else {
        panic!("expected Conflicts");
    }
    if let TransactionAttribute::NotaryAssisted(notary) = &parsed.attributes()[3] {
        assert_eq!(notary.nkeys, 3);
    } else {
        panic!("expected NotaryAssisted");
    }
    if let TransactionAttribute::OracleResponse(resp) = &parsed.attributes()[4] {
        assert_eq!(resp.id, 7);
        assert_eq!(resp.code, OracleResponseCode::Timeout);
        assert_eq!(resp.result, b"result");
    } else {
        panic!("expected OracleResponse");
    }
}

#[test]
fn parses_core_attributes_from_json() {
    let conflicts_hash = UInt256::zero().to_string();
    let conflicts_json = {
        let mut obj = JObject::new();
        obj.insert("type".to_string(), JToken::String("Conflicts".to_string()));
        obj.insert("hash".to_string(), JToken::String(conflicts_hash));
        obj
    };
    let conflicts = attribute_from_json(&conflicts_json).unwrap();
    assert!(matches!(conflicts, TransactionAttribute::Conflicts(_)));

    let mut nvb = JObject::new();
    nvb.insert(
        "type".to_string(),
        JToken::String("NotValidBefore".to_string()),
    );
    nvb.insert("height".to_string(), JToken::Number(5f64));
    let nvb_attr = attribute_from_json(&nvb).unwrap();
    assert!(matches!(nvb_attr, TransactionAttribute::NotValidBefore(_)));

    let mut notary = JObject::new();
    notary.insert(
        "type".to_string(),
        JToken::String("NotaryAssisted".to_string()),
    );
    notary.insert("nkeys".to_string(), JToken::Number(2f64));
    let notary_attr = attribute_from_json(&notary).unwrap();
    assert!(matches!(
        notary_attr,
        TransactionAttribute::NotaryAssisted(_)
    ));

    let mut oracle = JObject::new();
    oracle.insert(
        "type".to_string(),
        JToken::String("OracleResponse".to_string()),
    );
    oracle.insert("id".to_string(), JToken::Number(1f64));
    oracle.insert("code".to_string(), JToken::String("Timeout".to_string()));
    oracle.insert(
        "result".to_string(),
        JToken::String(BASE64.encode(b"hello")),
    );
    let oracle_attr = attribute_from_json(&oracle).unwrap();
    assert!(matches!(
        oracle_attr,
        TransactionAttribute::OracleResponse(_)
    ));
}

#[test]
fn stack_item_parses_array_and_struct() {
    let mut child = JObject::new();
    child.insert("type".to_string(), JToken::String("Integer".to_string()));
    child.insert("value".to_string(), JToken::String("5".to_string()));

    let array = JArray::from(vec![JToken::Object(child.clone())]);
    let mut array_obj = JObject::new();
    array_obj.insert("type".to_string(), JToken::String("Array".to_string()));
    array_obj.insert("value".to_string(), JToken::Array(array.clone()));

    let item_array = RpcUtility::stack_item_from_json(&array_obj).unwrap();
    let StackValue::Array(array_items) = item_array else {
        panic!("expected array");
    };
    assert_eq!(array_items.len(), 1);

    let mut struct_obj = JObject::new();
    struct_obj.insert("type".to_string(), JToken::String("Struct".to_string()));
    struct_obj.insert("value".to_string(), JToken::Array(array));
    let item_struct = RpcUtility::stack_item_from_json(&struct_obj).unwrap();
    let StackValue::Struct(struct_items) = item_struct else {
        panic!("expected struct");
    };
    assert_eq!(struct_items.len(), 1);
}

#[test]
fn stack_item_parses_map() {
    let mut key = JObject::new();
    key.insert("type".to_string(), JToken::String("ByteString".to_string()));
    key.insert(
        "value".to_string(),
        JToken::String(BASE64.encode("k".as_bytes())),
    );

    let mut value = JObject::new();
    value.insert("type".to_string(), JToken::String("Integer".to_string()));
    value.insert("value".to_string(), JToken::String("2".to_string()));

    let mut entry = JObject::new();
    entry.insert("key".to_string(), JToken::Object(key));
    entry.insert("value".to_string(), JToken::Object(value));

    let map_array = JArray::from(vec![JToken::Object(entry)]);
    let mut map_obj = JObject::new();
    map_obj.insert("type".to_string(), JToken::String("Map".to_string()));
    map_obj.insert("value".to_string(), JToken::Array(map_array));

    let item_map = RpcUtility::stack_item_from_json(&map_obj).unwrap();
    let StackValue::Map(map) = item_map else {
        panic!("expected map");
    };
    assert_eq!(map.len(), 1);
}

#[test]
fn stack_item_to_json_emits_array_and_map_shapes() {
    let array = StackValue::Array(vec![StackValue::Integer(5)]);
    assert_eq!(
        RpcUtility::stack_item_to_json(&array).unwrap().to_string(),
        r#"{"type":"Array","value":[{"type":"Integer","value":"5"}]}"#
    );

    let map = StackValue::Map(vec![(
        StackValue::ByteString(b"k".to_vec()),
        StackValue::Boolean(true),
    )]);
    let expected_map = concat!(
        r#"{"type":"Map","value":[{"key":{"type":"ByteString","value":"aw=="},"#,
        r#""value":{"type":"Boolean","value":true}}]}"#,
    );
    assert_eq!(
        RpcUtility::stack_item_to_json(&map).unwrap().to_string(),
        expected_map
    );
}

#[test]
fn parses_oracle_response_code_variants() {
    let string_code =
        parse_oracle_response_code(&JToken::String("ConsensusUnreachable".to_string())).unwrap();
    assert_eq!(string_code, OracleResponseCode::ConsensusUnreachable);

    let hex_code = parse_oracle_response_code(&JToken::String("0x1f".to_string())).unwrap();
    assert_eq!(hex_code, OracleResponseCode::ContentTypeNotSupported);

    let numeric = parse_oracle_response_code(&JToken::Number(0xff as f64)).unwrap();
    assert_eq!(numeric, OracleResponseCode::Error);

    let invalid = parse_oracle_response_code(&JToken::String("UnknownCode".to_string()));
    assert!(invalid.is_err());

    // round-trip string mapping
    assert_eq!(
        super::parsing::oracle_response_code_to_str(OracleResponseCode::Timeout),
        "Timeout"
    );
}
