use super::*;

#[test]
fn wallet_add_and_sign() {
    let mut wallet = Wallet::new();
    let account = Account::from_private_key(PrivateKey::new(hex!(
        "6d16ca2b9f10f8917ac12f90b91f864b0db1d0545d142e9d5b75f1c83c5f4321"
    )))
    .unwrap();
    let hash = account.script_hash();
    wallet.add_account(account).unwrap();
    assert_eq!(wallet.len(), 1);
    let signature = wallet.sign(&hash, b"payload").unwrap();
    assert_eq!(signature.0.len(), 64);
}

#[test]
fn wallet_keystore_roundtrip() {
    let mut wallet = Wallet::new();
    let account = Account::from_private_key(PrivateKey::new([9u8; 32])).unwrap();
    let hash = account.script_hash();
    wallet.add_account(account).unwrap();
    let keystore = wallet.to_keystore("pass").unwrap();
    assert_eq!(keystore.entries.len(), 1);
    let restored = Wallet::from_keystore(&keystore, "pass").unwrap();
    assert!(restored.account(&hash).is_some());
    assert_eq!(restored.len(), 1);
}

#[test]
fn wallet_nep6_roundtrip() {
    let mut wallet = Wallet::new();
    let mut account = Account::from_private_key(PrivateKey::new([1u8; 32])).unwrap();
    let hash = account.script_hash();
    account.set_default(true);
    let contract_hash =
        Hash160::from_slice(&hex!("17b24dbdc30b30f33d05a281a81f0c0a5f94b8c0")).unwrap();
    account.set_signer_scopes(SignerScopes::CALLED_BY_ENTRY | SignerScopes::CUSTOM_CONTRACTS);
    account.set_allowed_contracts(vec![contract_hash]);
    account.set_allowed_groups(vec![vec![0x02; 33]]);
    wallet.add_account(account).unwrap();

    let address_version = AddressVersion::MAINNET;
    let scrypt = ScryptParams { n: 2, r: 1, p: 1 };
    let nep6 = wallet
        .to_nep6_wallet("name", "1.0", super::NEP2_PASSWORD, scrypt, address_version)
        .unwrap();

    assert_eq!(nep6.accounts.len(), 1);
    let account_entry = &nep6.accounts[0];
    assert_eq!(account_entry.key.as_deref(), Some(super::NEP2_VECTOR));
    let signer = account_entry
        .extra
        .as_ref()
        .and_then(|extra| extra.get("signer"))
        .and_then(|value| value.as_object())
        .expect("signer extra");
    assert_eq!(
        signer.get("scopes").unwrap().as_str().unwrap(),
        "CalledByEntry|CustomContracts"
    );

    let restored =
        Wallet::from_nep6_wallet(&nep6, Some(super::NEP2_PASSWORD), address_version).unwrap();
    assert_eq!(restored.len(), 1);
    let restored_account = restored.account(&hash).unwrap();
    assert!(restored_account
        .signer_scopes()
        .contains(SignerScopes::CUSTOM_CONTRACTS));
    assert_eq!(restored_account.allowed_contracts(), &[contract_hash]);
    assert_eq!(restored_account.allowed_groups().len(), 1);
}
