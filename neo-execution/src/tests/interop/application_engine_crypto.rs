use super::*;
use crate::NoDiagnostic;
use crate::native_contract_provider::NoNativeContractProvider;
use neo_config::{Hardfork, ProtocolSettings};
use neo_crypto::Secp256r1Crypto;
use neo_payloads::{VerifiableContainer, Witness};
use neo_primitives::TriggerType;
use neo_storage::DataCache;
use neo_vm::OpCode;
use neo_vm::StackItem;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::script_builder::redeem_script::RedeemScript;
use std::sync::Arc;

fn engine_with_gorgon(active: bool) -> ApplicationEngine {
    let mut settings = ProtocolSettings::default();
    if active {
        settings.hardforks = settings.hardforks.with_activation(Hardfork::HfGorgon, 0);
    } else {
        settings.hardforks = settings.hardforks.without_activation(Hardfork::HfGorgon);
    }
    ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
        TriggerType::Application,
        Some(Arc::new(VerifiableContainer::from(
            neo_payloads::Transaction::new(),
        ))),
        Arc::new(DataCache::new(false)),
        None,
        settings,
        crate::application_engine::TEST_MODE_GAS,
        NoDiagnostic,
        Arc::new(NoNativeContractProvider),
    )
    .expect("application engine")
}

fn valid_public_key() -> Vec<u8> {
    hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
        .expect("public key hex")
}

fn out_of_field_public_key() -> Vec<u8> {
    // Compressed secp256r1 key with x = Q (the field prime). This is the
    // C# ArgumentException boundary that CheckSig/CheckMultisig must convert to
    // `false`, not a VM fault.
    hex::decode(concat!(
        "02",
        "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff"
    ))
    .expect("invalid public key hex")
}

fn load_test_context(engine: &mut ApplicationEngine) {
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load test script");
}

fn preverified_signature(
    message: &[u8; 36],
) -> (Arc<crate::PreverifiedSignatureCache>, Vec<u8>, [u8; 64]) {
    let private_key = [17u8; 32];
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive public key");
    let signature = Secp256r1Crypto::sign(message, &private_key).expect("sign message");
    let mut invocation = ScriptBuilder::new();
    invocation.emit_push(&signature);
    let witness = Witness::new_with_scripts(
        invocation.to_array(),
        RedeemScript::signature_redeem_script(&public_key),
    );
    let cache = crate::preverify_standard_witness_signatures(message, &witness)
        .expect("preverify canonical signature witness");
    (cache, public_key, signature)
}

#[test]
fn wrong_length_signature_returns_false_before_gorgon() {
    let engine = engine_with_gorgon(false);
    let public_key = valid_public_key();

    assert_eq!(
        engine.verify_signature(b"message", &public_key, &[0u8; 63]),
        Ok(false)
    );
}

#[test]
fn wrong_length_signature_faults_with_gorgon_configured_like_csharp_v3101() {
    let engine = engine_with_gorgon(true);
    let public_key = valid_public_key();

    assert!(
        engine
            .verify_signature(b"message", &public_key, &[0u8; 63])
            .is_err()
    );
}

#[test]
fn invalid_fixed_length_signature_returns_false() {
    let engine = engine_with_gorgon(true);
    let public_key = valid_public_key();

    assert_eq!(
        engine.verify_signature(b"message", &public_key, &[0u8; 64]),
        Ok(false)
    );
}

#[test]
fn cached_and_uncached_signature_checks_have_identical_results() {
    let message = [0x61; 36];
    let (cache, public_key, signature) = preverified_signature(&message);
    let uncached = engine_with_gorgon(true);
    let mut cached = engine_with_gorgon(true);
    cached.set_preverified_signature_cache(Arc::clone(&cache));

    assert_eq!(
        cached.verify_signature(&message, &public_key, &signature),
        uncached.verify_signature(&message, &public_key, &signature)
    );

    let mut invalid_signature = signature;
    invalid_signature[0] ^= 1;
    assert_eq!(
        cached.verify_signature(&message, &public_key, &invalid_signature),
        uncached.verify_signature(&message, &public_key, &invalid_signature)
    );
    assert_eq!(
        cache.metrics_snapshot(),
        crate::PreverifiedSignatureCacheMetricsSnapshot {
            canonical_uses: 0,
            lookups: 2,
            hits: 1,
            misses: 1,
        }
    );
}

#[test]
fn cache_does_not_bypass_malformed_key_or_gorgon_length_guards() {
    let message = [0x71; 36];
    let (cache, _, signature) = preverified_signature(&message);
    let mut engine = engine_with_gorgon(true);
    engine.set_preverified_signature_cache(Arc::clone(&cache));

    assert!(
        engine
            .verify_signature(&message, &[0x04; 33], &signature)
            .is_err()
    );
    assert!(
        engine
            .verify_signature(&message, &valid_public_key(), &[0u8; 63])
            .is_err()
    );
    assert_eq!(
        engine.verify_signature(&message, &out_of_field_public_key(), &signature),
        engine_with_gorgon(true)
            .verify_signature(&message, &out_of_field_public_key(), &signature,),
        "a cache miss must retain the canonical out-of-field false result"
    );
    let metrics = cache.metrics_snapshot();
    assert_eq!(metrics.lookups, 2);
    assert_eq!(metrics.hits, 0);
    assert_eq!(metrics.misses, 2);
}

#[test]
fn multisig_wrong_length_signature_keeps_pre_gorgon_false_result() {
    let mut engine = engine_with_gorgon(false);
    let public_key = valid_public_key();
    load_test_context(&mut engine);
    engine
        .push_array(vec![StackItem::from_byte_string(vec![0u8; 63])])
        .expect("push signatures");
    engine
        .push_array(vec![StackItem::from_byte_string(public_key)])
        .expect("push public keys");

    assert_eq!(engine.crypto_check_multisig(), Ok(false));
}

#[test]
fn multisig_wrong_length_signature_faults_after_gorgon() {
    let mut engine = engine_with_gorgon(true);
    let public_key = valid_public_key();
    load_test_context(&mut engine);
    engine
        .push_array(vec![StackItem::from_byte_string(vec![0u8; 63])])
        .expect("push signatures");
    engine
        .push_array(vec![StackItem::from_byte_string(public_key)])
        .expect("push public keys");

    assert!(engine.crypto_check_multisig().is_err());
}

#[test]
fn malformed_public_key_still_faults_before_and_after_gorgon() {
    let malformed_public_key = vec![0x02; 32];

    let pre_gorgon = engine_with_gorgon(false);
    assert!(
        pre_gorgon
            .verify_signature(b"message", &malformed_public_key, &[0u8; 63])
            .is_err()
    );

    let post_gorgon = engine_with_gorgon(true);
    assert!(
        post_gorgon
            .verify_signature(b"message", &malformed_public_key, &[0u8; 63])
            .is_err()
    );
}

#[test]
fn multisig_out_of_field_public_key_returns_false_instead_of_faulting() {
    let mut engine = engine_with_gorgon(true);
    let public_key = out_of_field_public_key();
    load_test_context(&mut engine);

    engine
        .push_array(vec![StackItem::from_byte_string(vec![0u8; 64])])
        .expect("push signatures");
    engine
        .push_array(vec![StackItem::from_byte_string(public_key)])
        .expect("push public keys");

    assert_eq!(engine.crypto_check_multisig(), Ok(false));
}
