use alloc::{collections::BTreeMap, vec::Vec};
#[cfg(feature = "std")]
use alloc::sync::Arc;

use neo_base::hash::Hash160;
use neo_crypto::SignatureBytes;
#[cfg(feature = "std")]
use neo_crypto::ecc256::PrivateKey;

use crate::{
    account::Account,
    error::WalletError,
    keystore::{decrypt_entry, Keystore},
};
#[cfg(feature = "std")]
use crate::keystore::{load_keystore, persist_keystore};
#[cfg(feature = "std")]
use neo_store::{ColumnId, Store};

#[derive(Default, Clone)]
pub struct Wallet {
    accounts: BTreeMap<Hash160, Account>,
}

impl Wallet {
    pub fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
        }
    }

    pub fn add_account(&mut self, account: Account) -> Result<(), WalletError> {
        if self.accounts.contains_key(&account.script_hash()) {
            return Err(WalletError::DuplicateAccount);
        }
        self.accounts.insert(account.script_hash(), account);
        Ok(())
    }

    pub fn remove_account(&mut self, hash: &Hash160) -> Result<(), WalletError> {
        self.accounts
            .remove(hash)
            .map(|_| ())
            .ok_or(WalletError::AccountNotFound)
    }

    pub fn account(&self, hash: &Hash160) -> Option<&Account> {
        self.accounts.get(hash)
    }

    pub fn sign(&self, hash: &Hash160, payload: &[u8]) -> Result<SignatureBytes, WalletError> {
        let account = self.accounts.get(hash).ok_or(WalletError::AccountNotFound)?;
        account.sign(payload)
    }

    pub fn to_keystore(&self, password: &str) -> Result<Keystore, WalletError> {
        let accounts: Vec<Account> = self.accounts.values().cloned().collect();
        Keystore::from_accounts(&accounts, password)
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    pub fn from_keystore(keystore: &Keystore, password: &str) -> Result<Self, WalletError> {
        let mut wallet = Wallet::new();
        for entry in &keystore.entries {
            let private = decrypt_entry(entry, password)?;
            let account = Account::from_private_key(private)?;
            if account.script_hash() != entry.script_hash {
                return Err(WalletError::IntegrityMismatch);
            }
            wallet.add_account(account)?;
        }
        Ok(wallet)
    }
}

#[cfg(feature = "std")]
pub struct WalletStorage<S: Store + ?Sized> {
    store: Arc<S>,
    column: ColumnId,
    key: Vec<u8>,
    keystore: Keystore,
}

#[cfg(feature = "std")]
impl<S: Store + ?Sized> WalletStorage<S> {
    pub fn open(store: Arc<S>, column: ColumnId, key: Vec<u8>) -> Result<Self, WalletError> {
        let keystore = load_keystore(store.as_ref(), column, key.as_slice())?.unwrap_or_default();
        Ok(Self {
            store,
            column,
            key,
            keystore,
        })
    }

    pub fn accounts(&self, password: &str) -> Result<Vec<Account>, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        Ok(wallet.accounts.values().cloned().collect())
    }

    pub fn script_hashes(&self) -> Vec<Hash160> {
        self.keystore
            .entries
            .iter()
            .map(|entry| entry.script_hash)
            .collect()
    }

    pub fn import_private_key(
        &mut self,
        private: PrivateKey,
        password: &str,
    ) -> Result<Account, WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        let account = Account::from_private_key(private)?;
        wallet.add_account(account.clone())?;
        self.store_wallet(wallet, password)?;
        Ok(account)
    }

    pub fn remove_account(
        &mut self,
        hash: &Hash160,
        password: &str,
    ) -> Result<(), WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        wallet.remove_account(hash)?;
        self.store_wallet(wallet, password)
    }

    pub fn sign(
        &self,
        hash: &Hash160,
        payload: &[u8],
        password: &str,
    ) -> Result<SignatureBytes, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        wallet.sign(hash, payload)
    }

    fn store_wallet(&mut self, wallet: Wallet, password: &str) -> Result<(), WalletError> {
        let keystore = wallet.to_keystore(password)?;
        persist_keystore(
            self.store.as_ref(),
            self.column,
            self.key.clone(),
            &keystore,
        )?;
        self.keystore = keystore;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;
    use neo_crypto::ecc256::PrivateKey;

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

    #[cfg(feature = "std")]
    #[test]
    fn wallet_storage_persists_accounts() {
        use neo_store::{ColumnId, MemoryStore};

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
}
