use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::contract::Contract;
use neo_core::smart_contract::helper::Helper as ContractHelper;
use neo_core::smart_contract::contract_parameter_type::ContractParameterType;
use neo_core::smart_contract::ContractParametersContext;
use neo_core::wallets::key_pair::KeyPair;
use neo_core::{Transaction, UInt160, WitnessScope};
use neo_vm::op_code::OpCode;
use std::sync::Arc;

use neo_core::persistence::DataCache;

fn make_tx_for_contract(contract_hash: UInt160) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(1);
    tx.set_system_fee(0);
    tx.set_network_fee(0);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_signers(vec![Signer::new(contract_hash, WitnessScope::GLOBAL)]);
    tx.set_attributes(Vec::new());
    tx.set_witnesses(Vec::new());
    tx
}

#[test]
fn test_multisig_invocation_ordering() {
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));

    let key1 = KeyPair::from_private_key(&[1u8; 32]).expect("key1");
    let key2 = KeyPair::from_private_key(&[2u8; 32]).expect("key2");
    let key3 = KeyPair::from_private_key(&[3u8; 32]).expect("key3");

    let pub1 = key1.get_public_key_point().expect("pub1");
    let pub2 = key2.get_public_key_point().expect("pub2");
    let pub3 = key3.get_public_key_point().expect("pub3");
    let contract = Contract::create_multi_sig_contract(2, &[pub1.clone(), pub2.clone(), pub3]);
    let (_, parsed_keys) =
        ContractHelper::parse_multi_sig_contract(&contract.script).expect("parse multisig");
    let pub2_bytes = pub2.encode_point(true).expect("pub2 bytes");
    assert!(parsed_keys.iter().any(|k| k.as_slice() == pub2_bytes.as_slice()));

    let mut tx = make_tx_for_contract(contract.script_hash());
    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");

    let mut context = ContractParametersContext::new(snapshot, tx.clone(), settings.network);
    assert!(context.script_hashes().contains(&contract.script_hash()));
    let sig2 = key2.sign(&sign_data).expect("sig2");
    let sig1 = key1.sign(&sign_data).expect("sig1");

    assert!(context
        .add_signature(contract.clone(), pub2, sig2)
        .expect("add signature"));
    assert!(context
        .add_signature(contract.clone(), pub1, sig1)
        .expect("add signature"));

    let witnesses = context.get_witnesses().expect("witnesses ready");
    tx.set_witnesses(witnesses);

    assert_eq!(
        tx.verify_state_independent(&settings),
        neo_core::ledger::VerifyResult::Succeed
    );
}

#[test]
fn test_multisig_rejects_non_member_signature() {
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));

    let key1 = KeyPair::from_private_key(&[4u8; 32]).expect("key1");
    let key2 = KeyPair::from_private_key(&[5u8; 32]).expect("key2");
    let pub1 = key1.get_public_key_point().expect("pub1");
    let pub2 = key2.get_public_key_point().expect("pub2");
    let contract = Contract::create_multi_sig_contract(2, &[pub1.clone(), pub2]);

    let tx = make_tx_for_contract(contract.script_hash());
    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");

    let key_other = KeyPair::from_private_key(&[6u8; 32]).expect("key_other");
    let pub_other = key_other.get_public_key_point().expect("pub_other");
    let sig_other = key_other.sign(&sign_data).expect("sig_other");

    let mut context = ContractParametersContext::new(snapshot, tx, settings.network);
    assert!(!context
        .add_signature(contract, pub_other, sig_other)
        .expect("add signature"));
    assert!(!context.completed());
}

#[test]
fn test_add_contract_requires_script_hash_in_context() {
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));

    let key1 = KeyPair::from_private_key(&[7u8; 32]).expect("key1");
    let key2 = KeyPair::from_private_key(&[8u8; 32]).expect("key2");
    let contract1 = Contract::create_signature_contract(
        key1.get_public_key_point().expect("pub1"),
    );
    let contract2 = Contract::create_signature_contract(
        key2.get_public_key_point().expect("pub2"),
    );

    let tx = make_tx_for_contract(contract1.script_hash());
    let mut context = ContractParametersContext::new(snapshot, tx, settings.network);
    assert!(!context.add_contract(contract2));
}

#[test]
fn test_single_signature_rejects_multiple_signature_parameters() {
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));

    let key = KeyPair::from_private_key(&[9u8; 32]).expect("key");
    let pub_key = key.get_public_key_point().expect("pub");
    let script = Contract::create_signature_contract(pub_key.clone()).script;
    let contract = Contract::create(
        vec![
            ContractParameterType::Signature,
            ContractParameterType::Signature,
        ],
        script,
    );

    let tx = make_tx_for_contract(contract.script_hash());
    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let sig = key.sign(&sign_data).expect("sig");

    let mut context = ContractParametersContext::new(snapshot, tx, settings.network);
    assert!(context
        .add_signature(contract, pub_key, sig)
        .is_err());
}
