use super::*;
use crate::enclave::EnclaveConfig;
use tempfile::tempdir;

fn setup_enclave() -> (tempfile::TempDir, Arc<TeeEnclave>) {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().join("enclave"),
        simulation: true,
        ..Default::default()
    };
    let enclave = Arc::new(TeeEnclave::new(config));
    enclave.initialize().unwrap();
    (temp, enclave)
}

#[test]
fn test_create_wallet() {
    let (temp, enclave) = setup_enclave();
    let wallet_path = temp.path().join("wallet");

    let wallet = TeeWallet::create(enclave, "test-wallet", &wallet_path).unwrap();
    assert_eq!(wallet.name(), "test-wallet");
    assert!(wallet.list_keys().is_empty());
}

#[test]
fn test_create_and_list_keys() {
    let (temp, enclave) = setup_enclave();
    let wallet_path = temp.path().join("wallet");

    let wallet = TeeWallet::create(enclave, "test-wallet", &wallet_path).unwrap();

    let key1 = wallet.create_key(Some("key1".to_string())).unwrap();
    let _key2 = wallet.create_key(Some("key2".to_string())).unwrap();

    let keys = wallet.list_keys();
    assert_eq!(keys.len(), 2);

    // First key should be default
    assert_eq!(
        wallet.default_account().unwrap().script_hash,
        key1.script_hash
    );
}

#[test]
fn test_sign_with_key() {
    let (temp, enclave) = setup_enclave();
    let wallet_path = temp.path().join("wallet");

    let wallet = TeeWallet::create(enclave, "test-wallet", &wallet_path).unwrap();
    let key = wallet.create_key(None).unwrap();

    let data = b"test data to sign";
    let signature = wallet.sign(&key.script_hash, data).unwrap();

    assert!(!signature.is_empty());
}
