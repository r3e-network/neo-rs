use neo_crypto::Crypto;
use neo_core::neo_io::Serializable;
use neo_core::network::p2p::payloads::{Signer, Witness};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract;
use neo_core::smart_contract::native::PolicyContract;
use neo_core::wallets::{helper::Helper, key_pair::KeyPair};
use neo_core::{Transaction, UInt160, WitnessScope};

const SAMPLE_PRIVATE_KEY: [u8; 32] = [1u8; 32];
const SAMPLE_WIF: &str = "KwFfNUhSDaASSAwtG7ssQM1uVX8RgX5GHWnnLfhfiQDigjioWXHH";
const SAMPLE_ADDRESS_HASH: &str = "6380ce3d7de7855bc5c1076d3b515eda380d2e90";
const SAMPLE_ADDRESS: &str = "AQqzr4WX3hUJrJp9aiFp3CstjcgbHSmDCA";

#[test]
fn script_hash_matches_reference() {
    let key = KeyPair::from_wif(SAMPLE_WIF).expect("failed to parse sample WIF");
    assert_eq!(key.private_key(), &SAMPLE_PRIVATE_KEY);
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
fn address_decode_errors_preserve_wallet_messages() {
    let invalid_base58 = Helper::to_script_hash("0", 0x17).expect_err("invalid base58");
    assert!(
        invalid_base58.starts_with("Invalid Base58 string: "),
        "unexpected invalid base58 error: {invalid_base58}"
    );

    let too_short = Helper::to_script_hash("1", 0x17).expect_err("too short");
    assert_eq!(
        too_short,
        "Invalid Base58Check format: decoded data length is too short (requires at least 4 checksum bytes)."
    );

    let mut invalid_checksum = SAMPLE_ADDRESS.to_string();
    invalid_checksum.pop();
    invalid_checksum.push('B');
    let checksum = Helper::to_script_hash(&invalid_checksum, 0x17).expect_err("bad checksum");
    assert_eq!(
        checksum,
        "Invalid Base58Check checksum: provided checksum does not match calculated checksum."
    );
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
    let digest = Crypto::sha256(b"System.Crypto.CheckSig");
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
    invocation.push(neo_vm_rs::OpCode::PUSHDATA1.byte());
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

    let expected_size = tx.size() as i64;
    let expected_fee = expected_size * PolicyContract::DEFAULT_FEE_PER_BYTE as i64
        + (PolicyContract::DEFAULT_EXEC_FEE_FACTOR as i64)
            * smart_contract::helper::Helper::signature_contract_cost();

    assert_eq!(fee, expected_fee);
}

#[test]
fn to_wif_roundtrip() {
    let key = KeyPair::from_wif(SAMPLE_WIF).expect("failed to parse sample WIF");
    let exported = key.to_wif();
    assert_eq!(exported, SAMPLE_WIF);
    let restored = KeyPair::from_wif(&exported).unwrap();
    assert_eq!(restored.private_key(), key.private_key());
}
