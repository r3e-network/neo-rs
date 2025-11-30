use neo_core::network::p2p::payloads::{Signer, Witness};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract;
use neo_core::smart_contract::native::PolicyContract;
use neo_core::wallets::{helper::Helper, key_pair::KeyPair};
use neo_core::{Transaction, UInt160, WitnessScope};
use sha2::{Digest, Sha256};

const SAMPLE_PRIVATE_KEY: [u8; 32] = [1u8; 32];
const SAMPLE_WIF: &str = "KwFfNUhSDaASSAwtG7ssQM1uVX8RgX5GHWnnLfhfiQDigjioWXHH";
const SAMPLE_ADDRESS_HASH: &str = "6380ce3d7de7855bc5c1076d3b515eda380d2e90";
const SAMPLE_ADDRESS: &str = "AQqzr4WX3hUJrJp9aiFp3CstjcgbHSmDCA";

#[test]
fn script_hash_matches_reference() {
    let key = KeyPair::from_wif(SAMPLE_WIF).expect("failed to parse sample WIF");
    assert_eq!(key.private_key(), SAMPLE_PRIVATE_KEY);
    let script_hash = key.get_script_hash();
    assert_eq!(hex::encode(script_hash.to_array()), SAMPLE_ADDRESS_HASH);
}

#[test]
fn address_conversion_roundtrip() {
    let script_hash = UInt160::from_bytes(&hex::decode(SAMPLE_ADDRESS_HASH).unwrap()).unwrap();
    let address = Helper::to_address(&script_hash, 0x17);

    let parsed = Helper::to_script_hash(&address, 0x17).expect("address decode");
    assert_eq!(hex::encode(parsed.to_array()), SAMPLE_ADDRESS_HASH);
    assert_eq!(address, SAMPLE_ADDRESS);
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
    let mut hasher = Sha256::new();
    hasher.update(b"System.Crypto.CheckSig");
    let digest = hasher.finalize();
    assert_eq!(
        &script[script.len() - 5..],
        [&[0x41], &digest[..4]].concat().as_slice()
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
        + (PolicyContract::DEFAULT_EXEC_FEE_FACTOR as i64)
            * smart_contract::helper::Helper::signature_contract_cost();

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

#[test]
fn to_wif_roundtrip() {
    let key = KeyPair::from_wif(SAMPLE_WIF).expect("failed to parse sample WIF");
    let exported = key.to_wif();
    assert_eq!(exported, SAMPLE_WIF);
    let restored = KeyPair::from_wif(&exported).unwrap();
    assert_eq!(restored.private_key(), key.private_key());
}
