use super::*;

#[test]
fn wallet_watch_only_account() {
    let mut wallet = Wallet::new();
    let hash = Hash160::from_slice(&[0x42; 20]).unwrap();
    wallet.add_watch_only(hash, None, false).unwrap();
    assert!(matches!(
        wallet.export_wif(&hash),
        Err(WalletError::WatchOnly)
    ));
}

#[test]
fn wallet_upgrades_watch_only_account() {
    let private = PrivateKey::new([9u8; 32]);
    let hash = Account::from_private_key(private.clone())
        .unwrap()
        .script_hash();
    let mut wallet = Wallet::new();
    wallet.add_watch_only(hash, None, false).unwrap();
    wallet.import_private_key(private, false).unwrap();
    assert!(!wallet.account(&hash).unwrap().is_watch_only());
}

#[test]
fn wallet_keystore_retains_watch_only_accounts() {
    let mut wallet = Wallet::new();
    let hash = Hash160::from_slice(&[0x33; 20]).unwrap();
    wallet.add_watch_only(hash, None, false).unwrap();
    let keystore = wallet.to_keystore("pass").unwrap();
    let restored = Wallet::from_keystore(&keystore, "pass").unwrap();
    assert!(restored.account(&hash).unwrap().is_watch_only());
}
