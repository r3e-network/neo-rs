use super::*;
use neo_config::ProtocolSettings;
use neo_execution::ApplicationEngine;
use neo_payloads::{Block, BlockHeader};
use neo_primitives::TriggerType;
use std::collections::HashMap;
use std::sync::Arc;

fn engine_for(
    trigger: TriggerType,
    snapshot: Arc<DataCache>,
    index: u32,
    settings: ProtocolSettings,
) -> ApplicationEngine {
    let mut header = BlockHeader::default();
    header.set_index(index);
    ApplicationEngine::new(
        trigger,
        None,
        snapshot,
        Some(Block::from_parts(header, vec![])),
        settings,
        0,
        None,
    )
    .expect("engine builds")
}

fn seed_committee_cache(cache: &DataCache, members: &[(ECPoint, BigInt)]) {
    cache.add(
        NeoToken::committee_key(),
        StorageItem::from_bytes(NeoToken::encode_committee(members).unwrap()),
    );
}

fn read_voter_reward(snapshot: &DataCache, pubkey: &ECPoint) -> Option<BigInt> {
    snapshot
        .get(&NeoToken::voter_reward_per_committee_key(pubkey))
        .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
}

fn gas_balance(snapshot: &DataCache, account: &UInt160) -> Option<BigInt> {
    let key = crate::GasToken::account_key(account);
    let item = snapshot.get(&key)?;
    let decoded =
        BinarySerializer::deserialize(&item.value_bytes(), &ExecutionEngineLimits::default(), None)
            .unwrap();
    let StackItem::Struct(fields) = decoded else {
        panic!("GAS account is not a struct");
    };
    Some(fields.items().first().unwrap().as_int().unwrap())
}

fn signature_address(pubkey: &ECPoint) -> UInt160 {
    UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()))
}

fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_idx = source.find(start).expect("start marker exists");
    let tail = &source[start_idx..];
    let end_idx = tail.find(end).expect("end marker exists");
    &tail[..end_idx]
}

#[test]
fn on_persist_refresh_recomputes_committee_and_emits_committee_changed() {
    // Single-member committee (every block refreshes); HF_Cockatrice at 0
    // so the notification path is active. Seeded: standby K1 cached,
    // turnout exactly at the 20% boundary, candidate K2 registered with 7
    // votes -> recompute elects [K2] and emits CommitteeChanged([K1],[K2]).
    let all = ProtocolSettings::default().standby_committee;
    let (k1, k2) = (all[0].clone(), all[1].clone());
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfCockatrice, 0u32);
    let settings = ProtocolSettings {
        standby_committee: vec![k1.clone()],
        validators_count: 1,
        hardforks,
        ..ProtocolSettings::default()
    };
    let cache = DataCache::new(false);
    seed_committee_cache(&cache, &[(k1.clone(), BigInt::from(0))]);
    cache.add(
        NeoToken::voters_count_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
    );
    cache.add(
        NeoToken::candidate_key(&k2),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(true, &BigInt::from(7)).unwrap()),
    );
    let snapshot = Arc::new(cache);

    let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 1, settings);
    NeoToken.on_persist(&mut engine).expect("on_persist");

    // The cache now holds the elected committee, CachedCommittee layout.
    let stored = snapshot
        .get(&NeoToken::committee_key())
        .unwrap()
        .value_bytes()
        .into_owned();
    assert_eq!(
        stored,
        NeoToken::encode_committee(&[(k2.clone(), BigInt::from(7))]).unwrap()
    );

    // CommitteeChanged([prev pubkeys], [new pubkeys]).
    let notes = engine.notifications();
    assert_eq!(notes.len(), 1, "exactly one notification");
    let note = &notes[0];
    assert_eq!(note.script_hash, NeoToken::script_hash());
    assert_eq!(note.event_name, "CommitteeChanged");
    assert_eq!(note.state.len(), 2);
    let keys_of = |item: &StackItem| -> Vec<Vec<u8>> {
        let StackItem::Array(array) = item else {
            panic!("CommitteeChanged arg is not an array");
        };
        array
            .items()
            .iter()
            .map(|i| i.as_bytes().unwrap().to_vec())
            .collect()
    };
    assert_eq!(keys_of(&note.state[0]), vec![k1.to_bytes()]);
    assert_eq!(keys_of(&note.state[1]), vec![k2.to_bytes()]);
}

#[test]
fn on_persist_refresh_without_cockatrice_updates_committee_silently() {
    // Same election as above, but HF_Cockatrice is unscheduled: the
    // committee cache still updates, with no notification (pre-3158
    // behavior, the C# hardfork gate).
    let all = ProtocolSettings::default().standby_committee;
    let (k1, k2) = (all[0].clone(), all[1].clone());
    let settings = ProtocolSettings {
        standby_committee: vec![k1.clone()],
        validators_count: 1,
        hardforks: HashMap::new(),
        ..ProtocolSettings::default()
    };
    let cache = DataCache::new(false);
    seed_committee_cache(&cache, &[(k1.clone(), BigInt::from(0))]);
    cache.add(
        NeoToken::voters_count_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
    );
    cache.add(
        NeoToken::candidate_key(&k2),
        StorageItem::from_bytes(NeoToken::encode_candidate_state(true, &BigInt::from(7)).unwrap()),
    );
    let snapshot = Arc::new(cache);

    let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 1, settings);
    NeoToken.on_persist(&mut engine).expect("on_persist");

    let stored = snapshot
        .get(&NeoToken::committee_key())
        .unwrap()
        .value_bytes()
        .into_owned();
    assert_eq!(
        stored,
        NeoToken::encode_committee(&[(k2, BigInt::from(7))]).unwrap()
    );
    assert!(
        engine.notifications().is_empty(),
        "no CommitteeChanged before Cockatrice"
    );
}

#[test]
fn on_persist_skips_recompute_off_refresh_blocks() {
    // m = 3, block index 2: 2 % 3 != 0, so the committee cache must stay
    // untouched even though a recompute would elect different members.
    let all = ProtocolSettings::default().standby_committee;
    let standby = all[..3].to_vec();
    let settings = ProtocolSettings {
        standby_committee: standby.clone(),
        validators_count: 1,
        ..ProtocolSettings::default()
    };
    let seeded: Vec<(ECPoint, BigInt)> = standby
        .iter()
        .map(|p| (p.clone(), BigInt::from(0)))
        .collect();
    let cache = DataCache::new(false);
    seed_committee_cache(&cache, &seeded);
    cache.add(
        NeoToken::voters_count_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
    );
    for (i, candidate) in all[3..6].iter().enumerate() {
        cache.add(
            NeoToken::candidate_key(candidate),
            StorageItem::from_bytes(
                NeoToken::encode_candidate_state(true, &BigInt::from(100 + i as i64)).unwrap(),
            ),
        );
    }
    let snapshot = Arc::new(cache);

    let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 2, settings);
    NeoToken.on_persist(&mut engine).expect("on_persist");

    let stored = snapshot
        .get(&NeoToken::committee_key())
        .unwrap()
        .value_bytes()
        .into_owned();
    assert_eq!(
        stored,
        NeoToken::encode_committee(&seeded).unwrap(),
        "cache untouched off refresh"
    );
    assert!(engine.notifications().is_empty());
}

/// Hand-computed C# PostPersistAsync values for the default settings
/// (m = 21, n = 7) with gasPerBlock = 5 GAS:
///   committee reward      = 5_0000_0000 * 10 / 100        = 0.5 GAS
///   voterRewardOfEachCommittee
///     = 5e8 * 80 * 1e8 * 21 / (21 + 7) / 100              = 3e16
///   member 0 (validator, factor 2, 1000 votes): 2*3e16/1000 = 6e13
///   member 7 (non-validator, factor 1, 400 votes): 3e16/400 = 7.5e13
#[test]
fn post_persist_committee_and_voter_rewards_match_csharp_math() {
    let settings = ProtocolSettings::default();
    assert_eq!(settings.committee_members_count(), 21);
    assert_eq!(settings.validators_count, 7);
    let members: Vec<(ECPoint, BigInt)> = settings
        .standby_committee
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let votes = match i {
                0 => 1000,
                7 => 400,
                _ => 0,
            };
            (p.clone(), BigInt::from(votes))
        })
        .collect();
    let cache = DataCache::new(false);
    seed_committee_cache(&cache, &members);
    NeoToken::new().put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
    // Pre-seed member 0's accumulator: C# `GetAndChange(key).Add(...)` is
    // read-modify-write, so the accrual must ADD to the existing value.
    cache.add(
        NeoToken::voter_reward_per_committee_key(&members[0].0),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(5))),
    );
    let snapshot = Arc::new(cache);

    // Block 0 is a refresh block (0 % 21 == 0).
    let mut engine = engine_for(
        TriggerType::PostPersist,
        Arc::clone(&snapshot),
        0,
        settings.clone(),
    );
    NeoToken.post_persist(&mut engine).expect("post_persist");

    // committee[0 % 21] earns 0.5 GAS at its signature address.
    let member0_addr = signature_address(&members[0].0);
    assert_eq!(
        gas_balance(&snapshot, &member0_addr),
        Some(BigInt::from(50_000_000))
    );
    // The mint emitted GAS Transfer(null, member0, 0.5 GAS).
    let transfer = engine
        .notifications()
        .iter()
        .find(|n| n.event_name == "Transfer")
        .expect("committee reward Transfer");
    assert_eq!(transfer.script_hash, crate::GasToken::script_hash());
    assert!(matches!(transfer.state[0], StackItem::Null));
    assert_eq!(
        transfer.state[1].as_bytes().unwrap().to_vec(),
        member0_addr.to_bytes()
    );
    assert_eq!(
        transfer.state[2].as_int().unwrap(),
        BigInt::from(50_000_000)
    );

    // Voter-reward accruals (zoomed by VoteFactor), added to any existing value.
    assert_eq!(
        read_voter_reward(&snapshot, &members[0].0),
        Some(BigInt::from(60_000_000_000_005i64)),
        "validator voter reward: pre-seeded 5 + 2 * 3e16 / 1000"
    );
    assert_eq!(
        read_voter_reward(&snapshot, &members[7].0),
        Some(BigInt::from(75_000_000_000_000i64)),
        "non-validator voter reward: 3e16 / 400"
    );
    assert_eq!(
        read_voter_reward(&snapshot, &members[1].0),
        None,
        "zero-vote members accrue nothing"
    );
}

#[test]
fn post_persist_off_refresh_blocks_only_mints_the_rotating_reward() {
    // Block 1 (1 % 21 != 0): committee[1] earns 0.5 GAS; no voter-reward
    // accrual happens even for members with votes.
    let settings = ProtocolSettings::default();
    let members: Vec<(ECPoint, BigInt)> = settings
        .standby_committee
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), BigInt::from(if i == 0 { 1000 } else { 0 })))
        .collect();
    let cache = DataCache::new(false);
    seed_committee_cache(&cache, &members);
    NeoToken::new().put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
    let snapshot = Arc::new(cache);

    let mut engine = engine_for(
        TriggerType::PostPersist,
        Arc::clone(&snapshot),
        1,
        settings.clone(),
    );
    NeoToken.post_persist(&mut engine).expect("post_persist");

    let member1_addr = signature_address(&members[1].0);
    assert_eq!(
        gas_balance(&snapshot, &member1_addr),
        Some(BigInt::from(50_000_000))
    );
    assert_eq!(
        gas_balance(&snapshot, &signature_address(&members[0].0)),
        None
    );
    assert_eq!(
        read_voter_reward(&snapshot, &members[0].0),
        None,
        "no accrual off refresh blocks"
    );
}

#[test]
fn post_persist_uses_cached_signature_account_for_rotating_committee_reward() {
    let source = include_str!("../../neo_token/mod.rs");
    let post_persist = slice_between(source, "fn post_persist", "fn invoke");
    assert!(
        post_persist.contains("candidate_signature_account(&member)"),
        "per-block committee reward should reuse cached signature accounts"
    );
    assert!(
        post_persist.contains("read_committee_member_at(&snapshot, member_index)"),
        "off-refresh blocks should read only the rotating committee member"
    );
    assert!(
        !post_persist.contains("create_signature_redeem_script(member.clone())"),
        "post_persist should not rebuild the same redeem script on every block"
    );
}

/// C# `NeoToken.OnManifestCompose` (NeoToken.cs:112-122): NEP-27 joins
/// NEP-17 once HF_Echidna is enabled at the height — and Echidna is a
/// manifest-refresh hardfork for NEO (C# carries it in `_usedHardforks`
/// via the Echidna-gated method registrations).
#[test]
fn manifest_standards_gain_nep27_at_echidna() {
    use neo_execution::native_contract::build_native_contract_state;

    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 10);
    let before = build_native_contract_state(&NeoToken, &settings, 9);
    assert_eq!(before.manifest.supported_standards, ["NEP-17"]);
    let after = build_native_contract_state(&NeoToken, &settings, 10);
    assert_eq!(after.manifest.supported_standards, ["NEP-17", "NEP-27"]);

    assert!(NativeContract::used_hardforks(&NeoToken).contains(&Hardfork::HfEchidna));
}
