use super::*;

#[test]
fn wallet_import_export_wif() {
    let mut wallet = Wallet::new();
    let hash = wallet.import_wif(super::WIF_VECTOR, true).unwrap();
    let exported = wallet.export_wif(&hash).unwrap();
    assert_eq!(exported, super::WIF_VECTOR);
    assert!(wallet.account(&hash).unwrap().is_default());
}

#[test]
fn wallet_import_export_nep2() {
    let mut wallet = Wallet::new();
    let private = PrivateKey::new([1u8; 32]);
    let hash = wallet.import_private_key(private, false).unwrap();
    let nep2 = wallet
        .export_nep2(
            &hash,
            super::NEP2_PASSWORD,
            ScryptParams { n: 2, r: 1, p: 1 },
            AddressVersion::MAINNET,
        )
        .unwrap();
    let mut restored = Wallet::new();
    let imported = restored
        .import_nep2(
            &nep2,
            super::NEP2_PASSWORD,
            ScryptParams { n: 2, r: 1, p: 1 },
            AddressVersion::MAINNET,
            false,
        )
        .unwrap();
    assert_eq!(imported, hash);
}
