use super::*;
use crate::enclave::EnclaveConfig;
use tempfile::tempdir;

#[test]
fn test_wallet_provider() {
    let temp = tempdir().unwrap();
    let config = EnclaveConfig {
        sealed_data_path: temp.path().join("enclave"),
        simulation: true,
        ..Default::default()
    };

    let enclave = Arc::new(TeeEnclave::new(config));
    enclave.initialize().unwrap();

    let provider = TeeWalletProvider::new(enclave).unwrap();

    let wallet_path = temp.path().join("my_wallet");
    let wallet = provider.create_wallet("test", &wallet_path).unwrap();

    assert!(TeeWalletProvider::is_tee_wallet(&wallet_path));

    // Reopen wallet
    drop(wallet);
    let _reopened = provider.open_wallet(&wallet_path).unwrap();
}
