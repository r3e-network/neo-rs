use alloc::sync::Arc;

use super::WalletStorage;
use crate::signer::SignerScopes;
use hex_literal::hex;
use neo_base::hash::Hash160;
use neo_crypto::ecc256::PrivateKey;
use neo_store::{ColumnId, MemoryStore};

#[test]
fn wallet_storage_persists_accounts() {
    let store = Arc::new(MemoryStore::new());
    let column = ColumnId::new("wallet");
    store.create_column(column);
    let key = b"primary".to_vec();

    let mut storage =
        WalletStorage::open(store.clone(), column, key.clone()).expect("open storage");
    storage
        .import_private_key(PrivateKey::new([5u8; 32]), "pass")
        .expect("import");
    assert_eq!(storage.accounts("pass").unwrap().len(), 1);

    let storage_again = WalletStorage::open(store, column, key).expect("reload");
    assert_eq!(storage_again.accounts("pass").unwrap().len(), 1);
}

#[test]
fn wallet_storage_persists_signer_metadata() {
    let store = Arc::new(MemoryStore::new());
    let column = ColumnId::new("wallet");
    store.create_column(column);
    let key = b"primary".to_vec();

    let mut storage =
        WalletStorage::open(store.clone(), column, key.clone()).expect("open storage");
    let account = storage
        .import_private_key(PrivateKey::new([7u8; 32]), "pass")
        .expect("import");
    let hash = account.script_hash();
    let contract = Hash160::from_slice(&hex!("17b24dbdc30b30f33d05a281a81f0c0a5f94b8c0")).unwrap();
    let group = vec![0x02; 33];

    storage
        .update_signer_metadata(
            &hash,
            "pass",
            SignerScopes::CALLED_BY_ENTRY
                | SignerScopes::CUSTOM_CONTRACTS
                | SignerScopes::CUSTOM_GROUPS,
            vec![contract],
            vec![group.clone()],
        )
        .expect("update metadata");

    drop(storage);

    let mut reopened =
        WalletStorage::open(store.clone(), column, key.clone()).expect("reload storage");
    let mut accounts = reopened.accounts("pass").expect("accounts");
    assert_eq!(accounts.len(), 1);
    let account = accounts.pop().unwrap();
    assert!(account
        .signer_scopes()
        .contains(SignerScopes::CUSTOM_CONTRACTS));
    assert!(account
        .signer_scopes()
        .contains(SignerScopes::CUSTOM_GROUPS));
    assert_eq!(account.allowed_contracts().len(), 1);
    assert_eq!(account.allowed_groups(), &[group.clone()]);

    reopened
        .update_signer_metadata(
            &hash,
            "pass",
            SignerScopes::CALLED_BY_ENTRY,
            Vec::new(),
            Vec::new(),
        )
        .expect("reset metadata");
    drop(reopened);

    let reset = WalletStorage::open(store, column, key).expect("reload after reset");
    let details = reset.account_details("pass").expect("details after reset");
    assert_eq!(details.len(), 1);
    let detail = &details[0];
    assert_eq!(detail.scopes, SignerScopes::CALLED_BY_ENTRY);
    assert!(detail.allowed_contracts.is_empty());
    assert!(detail.allowed_groups.is_empty());
}
