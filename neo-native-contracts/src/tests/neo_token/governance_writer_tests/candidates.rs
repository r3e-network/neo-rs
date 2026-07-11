use super::*;

/// A second valid secp256r1 public key, byte-wise distinct from
/// [`candidate_pubkey`] (a Neo N3 standby validator).
fn other_pubkey(index: u8) -> ECPoint {
    let keys = [
        "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
        "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
    ];
    ECPoint::from_bytes(&hex::decode(keys[usize::from(index)]).unwrap()).unwrap()
}

/// C# `GetCandidates`: `GetCandidatesInternal().Select(pubkey, votes)`
/// returns registered, non-blocked candidates only and caps the public array
/// after projection.
#[test]
fn get_candidates_filters_blocked_and_projects_votes() {
    let cache = DataCache::new(false);

    let kept = other_pubkey(0);
    cache.add(
        NeoToken::candidate_key(&kept),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(true, &BigInt::from(7)).unwrap()),
    );
    cache.add(
        NeoToken::candidate_key(&candidate_pubkey()),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(false, &BigInt::from(3)).unwrap()),
    );
    let blocked = other_pubkey(1);
    cache.add(
        NeoToken::candidate_key(&blocked),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(true, &BigInt::from(9)).unwrap()),
    );
    let blocked_account = UInt160::from_script(&Contract::create_signature_redeem_script(blocked));
    cache.add(
        crate::PolicyContract::blocked_account_key(&blocked_account),
        StorageItem::from_bytes(Vec::new()),
    );

    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(cache),
        None,
        ProtocolSettings::default(),
        10_000_000,
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
    )
    .expect("engine builds");

    let result = NativeContract::invoke(&NeoToken::new(), &mut engine, "getCandidates", &[])
        .expect("getCandidates succeeds");
    let decoded = BinarySerializer::deserialize(&result, &ExecutionEngineLimits::default(), None)
        .expect("serialized candidate array");
    let entries = decoded.as_array().expect("candidate array");

    assert_eq!(entries.len(), 1);
    let fields = entries[0].as_array().expect("Struct[pubkey, votes]");
    assert_eq!(
        fields[0].as_bytes().unwrap().as_slice(),
        kept.to_bytes().as_slice()
    );
    assert_eq!(fields[1].as_int().unwrap(), BigInt::from(7));
}

/// C# `GetAllCandidates`: the iterator yields `Struct[pubkey, votes]` per
/// registered candidate, skipping unregistered entries and candidates whose
/// signature-contract address PolicyContract blocks.
#[test]
fn get_all_candidates_iterator_filters_and_projects() {
    let cache = DataCache::new(false);
    let kept = other_pubkey(0);
    cache.add(
        NeoToken::candidate_key(&kept),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(true, &BigInt::from(7)).unwrap()),
    );
    cache.add(
        NeoToken::candidate_key(&candidate_pubkey()),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(false, &BigInt::from(3)).unwrap()),
    );
    let blocked = other_pubkey(1);
    cache.add(
        NeoToken::candidate_key(&blocked),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(true, &BigInt::from(9)).unwrap()),
    );
    let blocked_account = UInt160::from_script(&Contract::create_signature_redeem_script(blocked));
    cache.add(
        crate::PolicyContract::blocked_account_key(&blocked_account),
        StorageItem::from_bytes(Vec::new()),
    );

    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(cache),
        None,
        ProtocolSettings::default(),
        10_000_000,
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
    )
    .expect("engine builds");

    let result = NativeContract::invoke(&NeoToken::new(), &mut engine, "getAllCandidates", &[])
        .expect("getAllCandidates succeeds");
    let iterator_id = u32::from_le_bytes(result.try_into().expect("4-byte iterator id"));

    assert!(engine.iterator_next(iterator_id).expect("first next"));
    let StackItem::Struct(element) = engine.iterator_value(iterator_id).expect("value") else {
        panic!("iterator element must be a Struct[pubkey, votes]");
    };
    let items = element.items();
    assert_eq!(
        items[0].as_bytes().unwrap().as_slice(),
        kept.to_bytes().as_slice()
    );
    assert_eq!(items[1].as_int().unwrap(), BigInt::from(7));
    assert!(
        !engine.iterator_next(iterator_id).expect("exhausted"),
        "single element"
    );
}
