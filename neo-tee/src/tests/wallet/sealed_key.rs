use super::*;
use crate::enclave::EnclaveConfig;
use tempfile::tempdir;

fn sealed_key_with_script_hash(script_hash: [u8; 20]) -> SealedKey {
    SealedKey {
        sealed_data: SealedData {
            ciphertext: Vec::new(),
            nonce: [0; 12],
            aad: Vec::new(),
            counter: 0,
            version: SealedData::CURRENT_VERSION,
            context: None,
        },
        public_key: Vec::new(),
        label: None,
        script_hash,
        created_at: 0,
    }
}

#[test]
fn address_uses_base58_check_with_neo_n3_version() {
    let script_hash = [0xAB; 20];
    let sealed = sealed_key_with_script_hash(script_hash);
    let address = sealed.address();

    let mut payload = vec![0x35];
    payload.extend_from_slice(&script_hash);

    assert_eq!(address, Base58::encode_check(&payload));
    assert_eq!(Base58::decode_check(&address).unwrap(), payload);
}

#[test]
fn address_preserves_zero_script_hash_payload() {
    let sealed = sealed_key_with_script_hash([0; 20]);
    let address = sealed.address();

    let mut payload = vec![0x35];
    payload.extend_from_slice(&[0; 20]);

    assert_eq!(Base58::decode_check(&address).unwrap(), payload);
}

#[test]
fn test_seal_unseal_key() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);
    enclave.initialize().unwrap();

    let private_key = [0x42u8; 32];
    let public_key = [0x02u8; 33]; // Compressed public key format
    let script_hash = [0xABu8; 20];

    let sealed = SealedKey::seal(
        &enclave,
        &private_key,
        &public_key,
        &script_hash,
        Some("test-key".to_string()),
    )
    .unwrap();

    let unsealed = sealed.unseal(&enclave).unwrap();
    assert_eq!(&*unsealed, &private_key);
}

#[test]
fn test_save_load_sealed_key() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().to_path_buf(),
        simulation: true,
        ..Default::default()
    };

    let enclave = TeeEnclave::new(config);
    enclave.initialize().unwrap();

    let private_key = [0x42u8; 32];
    let public_key = [0x02u8; 33];
    let script_hash = [0xABu8; 20];

    let sealed =
        SealedKey::seal(&enclave, &private_key, &public_key, &script_hash, None).unwrap();

    let key_path = temp.path().join("test_key.json");
    sealed.save_to_file(&key_path).unwrap();

    let loaded = SealedKey::load_from_file(&key_path).unwrap();
    let unsealed = loaded.unseal(&enclave).unwrap();
    assert_eq!(&*unsealed, &private_key);
}
