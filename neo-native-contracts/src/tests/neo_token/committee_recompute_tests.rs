use super::*;
use neo_config::ProtocolSettings;

/// `n` distinct valid secp256r1 points (the mainnet standby committee).
fn points(n: usize) -> Vec<ECPoint> {
    let pts = ProtocolSettings::default().standby_committee;
    assert!(pts.len() >= n, "mainnet standby committee has 21 members");
    pts.into_iter().take(n).collect()
}

fn settings_with_committee(committee: Vec<ECPoint>) -> ProtocolSettings {
    ProtocolSettings {
        standby_committee: committee,
        validators_count: 1,
        ..ProtocolSettings::default()
    }
}

fn seed_voters_count(cache: &DataCache, value: i64) {
    cache.add(
        NeoToken::voters_count_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
    );
}

fn seed_candidate(cache: &DataCache, pubkey: &ECPoint, votes: i64) {
    cache.add(
        NeoToken::candidate_key(pubkey),
        StorageItem::from_bytes(
            NeoToken::encode_candidate_state(true, &BigInt::from(votes)).unwrap(),
        ),
    );
}

fn seed_committee_cache(cache: &DataCache, committee: &[ECPoint]) {
    let members: Vec<(ECPoint, BigInt)> = committee
        .iter()
        .cloned()
        .map(|point| (point, BigInt::from(0)))
        .collect();
    cache.add(
        NeoToken::committee_key(),
        StorageItem::from_bytes(NeoToken::encode_committee(&members).unwrap()),
    );
}

#[test]
fn should_refresh_committee_matches_csharp_modulo() {
    // C# `height % committeeMembersCount == 0`.
    assert!(NeoToken::should_refresh_committee(0, 21));
    assert!(!NeoToken::should_refresh_committee(1, 21));
    assert!(!NeoToken::should_refresh_committee(20, 21));
    assert!(NeoToken::should_refresh_committee(21, 21));
    assert!(NeoToken::should_refresh_committee(42, 21));
    // A single-member committee refreshes every block.
    assert!(NeoToken::should_refresh_committee(5, 1));
}

#[test]
fn standby_fallback_below_turnout_zips_registered_votes() {
    // Turnout one NEO short of the 20% boundary: votersCount * 5 =
    // 99_999_995 < TotalAmount, so even with >= m candidates the standby
    // committee wins — each member zipped with its registered-candidate
    // votes (zero when not a candidate). C#: `voterTurnout < 0.2M`.
    let all = points(6);
    let standby = all[..3].to_vec();
    let settings = settings_with_committee(standby.clone());
    let cache = DataCache::new(false);
    seed_voters_count(&cache, 19_999_999);
    seed_candidate(&cache, &standby[1], 42); // a standby member is a candidate
    seed_candidate(&cache, &standby[2], 77); // blocked standby candidate counts as zero
    let blocked_account = UInt160::from_script(&Contract::create_signature_redeem_script(
        standby[2].clone(),
    ));
    cache.add(
        crate::PolicyContract::blocked_account_key(&blocked_account),
        StorageItem::from_bytes(Vec::new()),
    );
    seed_candidate(&cache, &all[3], 1000);
    seed_candidate(&cache, &all[4], 900);
    seed_candidate(&cache, &all[5], 800);

    let members = NeoToken::new()
        .compute_committee_members(&cache, &settings)
        .unwrap();
    assert_eq!(
        members,
        vec![
            (standby[0].clone(), BigInt::from(0)),
            (standby[1].clone(), BigInt::from(42)),
            (standby[2].clone(), BigInt::from(0)),
        ],
        "standby order is preserved; votes come from non-blocked candidate records"
    );
}

#[test]
fn standby_fallback_when_fewer_candidates_than_committee() {
    // Turnout reached, but only 2 registered candidates for a 3-member
    // committee: C# `candidates.Length < settings.CommitteeMembersCount`
    // falls back to the standby committee.
    let all = points(5);
    let standby = all[..3].to_vec();
    let settings = settings_with_committee(standby.clone());
    let cache = DataCache::new(false);
    seed_voters_count(&cache, 20_000_000);
    seed_candidate(&cache, &all[3], 1000);
    seed_candidate(&cache, &all[4], 900);

    let members = NeoToken::new()
        .compute_committee_members(&cache, &settings)
        .unwrap();
    let keys: Vec<ECPoint> = members.into_iter().map(|(p, _)| p).collect();
    assert_eq!(keys, standby);
}

#[test]
fn top_m_at_exact_turnout_boundary_orders_votes_desc_pubkey_asc() {
    // votersCount * 5 == TotalAmount exactly: C# `voterTurnout < 0.2M` is
    // false (>= 0.2 passes), so with enough candidates the elected
    // committee is the top m by (votes DESC, pubkey ASC).
    let all = points(5);
    let standby = all[..3].to_vec();
    let settings = settings_with_committee(standby);
    let cache = DataCache::new(false);
    seed_voters_count(&cache, 20_000_000);
    let (c0, c1, c2, c3) = (&all[1], &all[2], &all[3], &all[4]);
    seed_candidate(&cache, c0, 10);
    seed_candidate(&cache, c1, 7);
    seed_candidate(&cache, c2, 50);
    seed_candidate(&cache, c3, 5); // 4th candidate drops out of the top 3

    let members = NeoToken::new()
        .compute_committee_members(&cache, &settings)
        .unwrap();
    assert_eq!(
        members,
        vec![
            (c2.clone(), BigInt::from(50)),
            (c0.clone(), BigInt::from(10)),
            (c1.clone(), BigInt::from(7)),
        ]
    );
}

#[test]
fn top_m_breaks_vote_ties_by_ascending_pubkey() {
    // C# `OrderByDescending(votes).ThenBy(pubkey)` — equal votes order by
    // the ECPoint comparison (X then Y), ascending.
    let all = points(4);
    let standby = vec![all[0].clone()];
    let settings = settings_with_committee(standby);
    let cache = DataCache::new(false);
    seed_voters_count(&cache, 20_000_000);
    let (a, b) = (all[2].clone(), all[3].clone());
    seed_candidate(&cache, &a, 9);
    seed_candidate(&cache, &b, 9);

    let members = NeoToken::new()
        .compute_committee_members(&cache, &settings)
        .unwrap();
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    assert_eq!(
        members,
        vec![(lo, BigInt::from(9))],
        "m = 1 takes the lower pubkey"
    );
    drop(hi);
}

#[test]
fn bft_address_uses_the_bft_multisig_threshold() {
    // C# Contract.GetBFTAddress: m = n - (n - 1) / 3 (7 validators -> 5).
    let validators = ProtocolSettings::default().standby_validators();
    assert_eq!(validators.len(), 7);
    let script =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            5,
            &validators,
        )
        .unwrap();
    assert_eq!(
        NeoToken::bft_address(&validators).unwrap(),
        UInt160::from_script(&script)
    );
}

#[test]
fn next_consensus_address_recomputes_validators_only_on_refresh_height() {
    let all = points(6);
    let standby = all[..3].to_vec();
    let settings = settings_with_committee(standby.clone());
    let cache = DataCache::new(false);
    seed_committee_cache(&cache, &standby);
    seed_voters_count(&cache, 20_000_000);
    seed_candidate(&cache, &all[3], 100);
    seed_candidate(&cache, &all[4], 50);
    seed_candidate(&cache, &all[5], 25);

    let cached_validator = vec![standby[0].clone()];
    let elected_validator = vec![all[3].clone()];

    assert_eq!(
        NeoToken::new()
            .next_consensus_address_for_block(&cache, &settings, 1)
            .unwrap(),
        NeoToken::bft_address(&cached_validator).unwrap(),
        "off-refresh blocks use cached GetNextBlockValidators"
    );
    assert_eq!(
        NeoToken::new()
            .next_consensus_address_for_block(&cache, &settings, 3)
            .unwrap(),
        NeoToken::bft_address(&elected_validator).unwrap(),
        "refresh blocks use ComputeNextBlockValidators"
    );
}
