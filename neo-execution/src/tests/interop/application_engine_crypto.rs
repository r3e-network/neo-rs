use super::*;
use crate::NoDiagnostic;
use crate::native_contract_provider::NoNativeContractProvider;
use neo_config::{Hardfork, ProtocolSettings};
use neo_payloads::VerifiableContainer;
use neo_primitives::TriggerType;
use neo_storage::DataCache;
use neo_vm::StackItem;
use neo_vm_rs::OpCode;
use std::sync::Arc;

fn engine_with_gorgon(active: bool) -> ApplicationEngine {
    let mut settings = ProtocolSettings::default();
    if active {
        settings.hardforks.insert(Hardfork::HfGorgon, 0);
    } else {
        settings.hardforks.remove(&Hardfork::HfGorgon);
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
