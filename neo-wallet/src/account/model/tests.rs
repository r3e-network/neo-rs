use super::*;
use hex_literal::hex;
use neo_crypto::ecc256::PrivateKey;

#[test]
fn account_from_private_key() {
    let private = PrivateKey::new(hex!(
        "c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75"
    ));
    let account = Account::from_private_key(private.clone()).expect("account");
    assert!(!account.is_watch_only());
    let signature = account.sign(b"neo-wallet").expect("signature");
    assert_eq!(signature.0.len(), 64);
    assert!(account.label().is_none());
    assert!(account.contract().is_some());
}

#[test]
fn watch_only_account() {
    let private = PrivateKey::new([1u8; 32]);
    let account = Account::from_private_key(private.clone()).unwrap();
    let watch = Account::watch_only(account.public_key().unwrap().clone());
    assert!(watch.is_watch_only());
    assert!(watch.sign(&[1, 2, 3]).is_err());
    assert!(watch.contract().is_some());
    let signer = watch.to_signer();
    assert_eq!(signer.scopes(), SignerScopes::CALLED_BY_ENTRY);
}

#[test]
fn update_signer_metadata_requires_contracts_with_scope() {
    let mut account = Account::from_private_key(PrivateKey::new([2u8; 32])).expect("account");
    let result =
        account.update_signer_metadata(SignerScopes::CUSTOM_CONTRACTS, Vec::new(), Vec::new());
    assert!(matches!(result, Err(WalletError::InvalidSignerMetadata(_))));

    let contract = Hash160::from_slice(&hex!("0102030405060708090a0b0c0d0e0f1011121314")).unwrap();
    account
        .update_signer_metadata(SignerScopes::CUSTOM_CONTRACTS, vec![contract], Vec::new())
        .expect("metadata applied");
}

#[test]
fn update_signer_metadata_validates_groups() {
    let mut account = Account::from_private_key(PrivateKey::new([3u8; 32])).expect("account");
    let invalid_group = vec![0u8; 32];
    let result = account.update_signer_metadata(
        SignerScopes::CUSTOM_GROUPS,
        Vec::new(),
        vec![invalid_group],
    );
    assert!(matches!(result, Err(WalletError::InvalidSignerMetadata(_))));
}
