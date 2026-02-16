use neo_core::IVerifiable;
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::wallets::KeyPair;
use neo_vm::StackItem;
use neo_vm::op_code::OpCode;
use std::sync::Arc;

fn sample_transaction() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_valid_until_block(1);
    tx
}

fn make_engine(tx: Transaction, settings: ProtocolSettings) -> ApplicationEngine {
    let snapshot = Arc::new(DataCache::new(false));
    let container: Arc<dyn IVerifiable> = Arc::new(tx);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        settings,
        200_000_000,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::NONE, None)
        .expect("load script");
    engine
}

#[test]
fn crypto_checksig_accepts_uncompressed_pubkey() {
    let settings = ProtocolSettings::default();
    let tx = sample_transaction();
    let message = get_sign_data_vec(&tx, settings.network).expect("sign data");

    let key = KeyPair::from_private_key(&[1u8; 32]).expect("keypair");
    let signature = key.sign(&message).expect("sign");
    let pubkey = key.public_key(); // uncompressed 65-byte SEC1

    let mut engine = make_engine(tx.clone(), settings.clone());
    engine
        .push(StackItem::from_byte_string(signature))
        .expect("push signature");
    engine
        .push(StackItem::from_byte_string(pubkey))
        .expect("push pubkey");
    assert!(engine.crypto_check_sig().expect("check sig"));

    let mut bad_engine = make_engine(tx, settings);
    bad_engine
        .push(StackItem::from_byte_string(vec![0xAA; 64]))
        .expect("push signature");
    bad_engine
        .push(StackItem::from_byte_string(vec![0xBB; 70]))
        .expect("push bad pubkey");
    assert!(bad_engine.crypto_check_sig().is_err());
}

#[test]
fn crypto_checkmultisig_behaves_like_csharp() {
    let settings = ProtocolSettings::default();
    let tx = sample_transaction();
    let message = get_sign_data_vec(&tx, settings.network).expect("sign data");

    let key1 = KeyPair::from_private_key(&[1u8; 32]).expect("key1");
    let key2 = KeyPair::from_private_key(&[2u8; 32]).expect("key2");

    let signature1 = key1.sign(&message).expect("sign1");
    let signature2 = key2.sign(&message).expect("sign2");

    let pubkeys = StackItem::from_array(vec![
        StackItem::from_byte_string(key1.public_key()),
        StackItem::from_byte_string(key2.public_key()),
    ]);
    let signatures = StackItem::from_array(vec![
        StackItem::from_byte_string(signature1.clone()),
        StackItem::from_byte_string(signature2.clone()),
    ]);

    let mut engine = make_engine(tx.clone(), settings.clone());
    engine.push(signatures).expect("push signatures");
    engine.push(pubkeys).expect("push pubkeys");
    assert!(engine.crypto_check_multisig().expect("check multisig"));

    let mut empty_pubkeys = make_engine(tx.clone(), settings.clone());
    empty_pubkeys
        .push(StackItem::from_array(vec![StackItem::from_byte_string(
            signature1.clone(),
        )]))
        .expect("push signatures");
    empty_pubkeys
        .push(StackItem::from_array(Vec::new()))
        .expect("push pubkeys");
    assert!(empty_pubkeys.crypto_check_multisig().is_err());

    let mut empty_signatures = make_engine(tx.clone(), settings.clone());
    empty_signatures
        .push(StackItem::from_array(Vec::new()))
        .expect("push signatures");
    empty_signatures
        .push(StackItem::from_array(vec![StackItem::from_byte_string(
            key1.public_key(),
        )]))
        .expect("push pubkeys");
    assert!(empty_signatures.crypto_check_multisig().is_err());

    let mut invalid_signature = make_engine(tx.clone(), settings.clone());
    invalid_signature
        .push(StackItem::from_array(vec![
            StackItem::from_byte_string(signature1),
            StackItem::from_byte_string(vec![0u8; 64]),
        ]))
        .expect("push signatures");
    invalid_signature
        .push(StackItem::from_array(vec![
            StackItem::from_byte_string(key1.public_key()),
            StackItem::from_byte_string(key2.public_key()),
        ]))
        .expect("push pubkeys");
    assert!(
        !invalid_signature
            .crypto_check_multisig()
            .expect("check multisig")
    );

    let mut invalid_pubkey = make_engine(tx, settings);
    invalid_pubkey
        .push(StackItem::from_array(vec![StackItem::from_byte_string(
            signature2,
        )]))
        .expect("push signatures");
    invalid_pubkey
        .push(StackItem::from_array(vec![StackItem::from_byte_string(
            vec![0xCC; 70],
        )]))
        .expect("push pubkeys");
    assert!(invalid_pubkey.crypto_check_multisig().is_err());
}
