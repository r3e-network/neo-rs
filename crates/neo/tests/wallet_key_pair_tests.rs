use neo_core::network::p2p::payloads::{Signer, Witness};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract;
use neo_core::smart_contract::native::PolicyContract;
use neo_core::wallets::{helper::Helper, key_pair::KeyPair};
use neo_core::{Transaction, UInt160, WitnessScope};

const SAMPLE_WIF: &str = "L5oLkpSp25PEDoB9FsVpjiqSgu7x3t3GBnWFdYbEDuVwvw9THqsQ";
const SAMPLE_ADDRESS_HASH: &str = "23ba2703c53263e8d6e522dc32203339dcd8eee9";
const SAMPLE_ADDRESS: &str = "AK2nJJpJr6o664CWJKi1QRXjqeic2zRp8y";

#[test]
fn script_hash_matches_reference() {
    let key = KeyPair::from_wif(SAMPLE_WIF).expect("failed to parse sample WIF");
    let script_hash = key.get_script_hash();
    assert_eq!(hex::encode(script_hash.to_array()), SAMPLE_ADDRESS_HASH);
}

#[test]
fn address_conversion_roundtrip() {
    let script_hash = UInt160::from_bytes(&hex::decode(SAMPLE_ADDRESS_HASH).unwrap()).unwrap();
    let address = Helper::to_address(&script_hash, 0x17);
    assert_eq!(address, SAMPLE_ADDRESS);

    let parsed = Helper::to_script_hash(&address, 0x17).expect("address decode");
    assert_eq!(hex::encode(parsed.to_array()), SAMPLE_ADDRESS_HASH);
}

#[test]
fn verification_script_matches_contract_helper() {
    let key = KeyPair::from_wif(SAMPLE_WIF).expect("failed to parse sample WIF");
    let script = key.get_verification_script();

    // Contract hash derived from verification script must match script hash
    let hash = UInt160::from_script(&script);
    assert_eq!(hex::encode(hash.to_array()), SAMPLE_ADDRESS_HASH);

    // Script should begin with PUSHDATA1 (0x0c) and end with CheckSig syscall hash
    assert_eq!(script[0], 0x0c);
    assert_eq!(script[1] as usize, key.compressed_public_key().len());
    assert_eq!(
        &script[script.len() - 5..script.len() - 1],
        b"\x41\x6d\xdf\x06"
    ); // System.Crypto.CheckSig hash
}

#[test]
fn calculate_network_fee_for_standard_signature() {
    let key = KeyPair::from_wif(SAMPLE_WIF).expect("failed to parse sample WIF");
    let script_hash = key.get_script_hash();

    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(script_hash, WitnessScope::GLOBAL)]);

    let mut invocation = Vec::new();
    invocation.push(neo_vm::op_code::OpCode::PUSHDATA1 as u8);
    invocation.push(64);
    invocation.extend_from_slice(&[0u8; 64]);

    let verification =
        smart_contract::helper::Helper::signature_redeem_script(&key.compressed_public_key());

    tx.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);

    let snapshot = DataCache::new(true);
    let settings = ProtocolSettings::default();

    let fee = Helper::calculate_network_fee(
        &tx,
        &snapshot,
        &settings,
        None,
        smart_contract::application_engine::TEST_MODE_GAS,
    )
    .expect("network fee");

    let expected_size = 67 + var_size_with_payload_test(verification_script_len(&tx));
    let expected_fee = expected_size * PolicyContract::DEFAULT_FEE_PER_BYTE as i64
        + (PolicyContract::DEFAULT_EXEC_FEE_FACTOR as i64) * signature_contract_cost_test();

    assert_eq!(fee, expected_fee);
}

fn verification_script_len(tx: &Transaction) -> usize {
    tx.witnesses()[0].verification_script.len()
}

fn var_size_with_payload_test(len: usize) -> i64 {
    if len < 0xFD {
        1 + len as i64
    } else if len <= 0xFFFF {
        3 + len as i64
    } else if len <= 0xFFFF_FFFF {
        5 + len as i64
    } else {
        9 + len as i64
    }
}

fn signature_contract_cost_test() -> i64 {
    let push_data_price = 1 << 3;
    let syscall_price = 0;
    push_data_price * 2 + syscall_price + smart_contract::application_engine::CHECK_SIG_PRICE
}

#[test]
fn to_wif_roundtrip() {
    let key = KeyPair::from_wif(SAMPLE_WIF).expect("failed to parse sample WIF");
    let exported = key.to_wif();
    assert_eq!(exported, SAMPLE_WIF);
}
