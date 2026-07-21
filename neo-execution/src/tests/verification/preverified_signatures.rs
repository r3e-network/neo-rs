use super::*;

use neo_crypto::Secp256r1Crypto;
use neo_vm::script_builder::ScriptBuilder;

fn sign_data(fill: u8) -> [u8; NEO_SIGN_DATA_LEN] {
    [fill; NEO_SIGN_DATA_LEN]
}

fn invocation(signature: &[u8; RAW_SIGNATURE_LEN]) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push(signature);
    builder.to_array()
}

fn signature_witness(
    sign_data: &[u8],
    private_key: &[u8; 32],
) -> (Witness, Vec<u8>, [u8; RAW_SIGNATURE_LEN]) {
    let public_key = Secp256r1Crypto::derive_public_key(private_key).expect("derive public key");
    let signature = Secp256r1Crypto::sign(sign_data, private_key).expect("sign data");
    let witness = Witness::new_with_scripts(
        invocation(&signature),
        RedeemScript::signature_redeem_script(&public_key),
    );
    (witness, public_key, signature)
}

#[test]
fn cache_is_bound_to_exact_message_public_key_and_signature_bytes() {
    let message = sign_data(0x21);
    let (witness, public_key, signature) = signature_witness(&message, &[7u8; 32]);
    let cache = preverify_standard_witness_signatures(&message, &witness)
        .expect("canonical signature witness");

    assert_eq!(cache.operation_count(), 1);
    assert_eq!(cache.lookup(&message, &public_key, &signature), Some(true));

    let mut other_message = message;
    other_message[0] ^= 1;
    assert_eq!(cache.lookup(&other_message, &public_key, &signature), None);

    let other_public_key =
        Secp256r1Crypto::derive_public_key(&[8u8; 32]).expect("derive second public key");
    assert_eq!(cache.lookup(&message, &other_public_key, &signature), None);

    let mut other_signature = signature;
    other_signature[0] ^= 1;
    assert_eq!(cache.lookup(&message, &public_key, &other_signature), None);

    assert_eq!(
        cache.metrics_snapshot(),
        PreverifiedSignatureCacheMetricsSnapshot {
            canonical_uses: 0,
            lookups: 4,
            hits: 1,
            misses: 3,
        }
    );
}

#[test]
fn invalid_signature_outcome_is_cached_as_false() {
    let signed_message = sign_data(0x31);
    let checked_message = sign_data(0x32);
    let (witness, public_key, signature) = signature_witness(&signed_message, &[9u8; 32]);

    let cache = preverify_standard_witness_signatures(&checked_message, &witness)
        .expect("canonical witness with a false crypto outcome");
    assert_eq!(
        cache.lookup(&checked_message, &public_key, &signature),
        Some(false)
    );
}

#[test]
fn malformed_or_noncanonical_inputs_do_not_produce_a_cache() {
    let message = sign_data(0x41);
    let (witness, _, _) = signature_witness(&message, &[10u8; 32]);

    assert!(preverify_standard_witness_signatures(&message[..35], &witness).is_none());

    let malformed_invocation =
        Witness::new_with_scripts(vec![0x01], witness.verification_script.clone());
    assert!(preverify_standard_witness_signatures(&message, &malformed_invocation).is_none());

    let unsupported_script = Witness::new_with_scripts(
        witness.invocation_script,
        vec![neo_vm::OpCode::PUSH1.byte()],
    );
    assert!(preverify_standard_witness_signatures(&message, &unsupported_script).is_none());
}

#[test]
fn multisig_preverification_mirrors_canonical_reverse_pop_order() {
    let message = sign_data(0x49);
    let key_pairs = [[41u8; 32], [42u8; 32], [43u8; 32], [44u8; 32]]
        .into_iter()
        .map(|private_key| {
            let public_key =
                Secp256r1Crypto::derive_public_key(&private_key).expect("derive multisig key");
            (public_key, private_key)
        })
        .collect::<Vec<_>>();
    let public_keys = key_pairs
        .iter()
        .map(|(public_key, _)| public_key.clone())
        .collect::<Vec<_>>();
    let verification_script = RedeemScript::multi_sig_redeem_script_from_keys(2, &public_keys)
        .expect("build canonical 2-of-4 script");
    let (_, sorted_public_keys) = RedeemScript::parse_multi_sig_contract(&verification_script)
        .expect("parse canonical multisig");
    let signatures = [0usize, 2].map(|key_index| {
        let public_key = &sorted_public_keys[key_index];
        let private_key = key_pairs
            .iter()
            .find_map(|(candidate, private_key)| (candidate == public_key).then_some(private_key))
            .expect("private key for sorted public key");
        Secp256r1Crypto::sign(&message, private_key).expect("sign multisig message")
    });
    let mut invocation = ScriptBuilder::new();
    for signature in &signatures {
        invocation.emit_push(signature);
    }
    let witness = Witness::new_with_scripts(invocation.to_array(), verification_script);

    let cache = preverify_standard_witness_signatures(&message, &witness)
        .expect("canonical multisig cache");
    assert_eq!(cache.operation_count(), 4);

    // NeoVM pops sig[1] and keys[3..] first, then sig[0] and keys[1..].
    assert_eq!(
        cache.lookup(&message, &sorted_public_keys[3], &signatures[1]),
        Some(false)
    );
    assert_eq!(
        cache.lookup(&message, &sorted_public_keys[2], &signatures[1]),
        Some(true)
    );
    assert_eq!(
        cache.lookup(&message, &sorted_public_keys[1], &signatures[0]),
        Some(false)
    );
    assert_eq!(
        cache.lookup(&message, &sorted_public_keys[0], &signatures[0]),
        Some(true)
    );
    let metrics = cache.metrics_snapshot();
    assert_eq!(metrics.lookups, 4);
    assert_eq!(metrics.hits, 4);
    assert_eq!(metrics.misses, 0);
}

#[test]
fn oversized_multisig_returns_a_partial_cache_and_leaves_later_pairs_uncached() {
    let message = sign_data(0x51);
    let mut key_pairs = Vec::with_capacity(MAX_PREVERIFIED_SIGNATURES + 1);
    for index in 1..=MAX_PREVERIFIED_SIGNATURES + 1 {
        let mut private_key = [0u8; 32];
        private_key[28..].copy_from_slice(&(index as u32).to_be_bytes());
        let public_key =
            Secp256r1Crypto::derive_public_key(&private_key).expect("derive multisig key");
        key_pairs.push((public_key, private_key));
    }

    let public_keys = key_pairs
        .iter()
        .map(|(public_key, _)| public_key.clone())
        .collect::<Vec<_>>();
    let verification_script = RedeemScript::multi_sig_redeem_script_from_keys(1, &public_keys)
        .expect("build canonical multisig");
    let (_, sorted_public_keys) = RedeemScript::parse_multi_sig_contract(&verification_script)
        .expect("parse canonical multisig");
    let final_canonical_public_key = sorted_public_keys.first().expect("first public key");
    let final_canonical_private_key = key_pairs
        .iter()
        .find_map(|(public_key, private_key)| {
            (public_key == final_canonical_public_key).then_some(private_key)
        })
        .expect("private key for final canonical public key");
    let signature =
        Secp256r1Crypto::sign(&message, final_canonical_private_key).expect("sign multisig");
    let witness = Witness::new_with_scripts(invocation(&signature), verification_script);

    let cache = preverify_standard_witness_signatures(&message, &witness)
        .expect("bounded partial multisig cache");
    assert_eq!(cache.operation_count(), MAX_PREVERIFIED_SIGNATURES);
    assert_eq!(
        cache.lookup(&message, final_canonical_public_key, &signature),
        None
    );
}
