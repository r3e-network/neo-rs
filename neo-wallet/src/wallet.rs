#[cfg(feature = "std")]
use alloc::sync::Arc;
use alloc::{collections::BTreeMap, string::String, vec::Vec};

use neo_base::encoding::{WifDecode, WifEncode};
use neo_base::{hash::Hash160, AddressVersion};
use neo_crypto::ecc256::PrivateKey;
use neo_crypto::{nep2::encrypt_nep2, scrypt::ScryptParams, SignatureBytes};

#[cfg(feature = "std")]
use crate::keystore::{load_keystore, persist_keystore};
#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};
use crate::{
    account::{Account, Contract},
    error::WalletError,
    keystore::{decrypt_entry, Keystore},
    nep6::Nep6Wallet,
    signer::SignerScopes,
};
#[cfg(feature = "std")]
use neo_store::{ColumnId, Store};

#[derive(Default, Clone)]
pub struct Wallet {
    accounts: BTreeMap<Hash160, Account>,
}

#[derive(Clone, Debug)]
pub struct AccountDetails {
    pub script_hash: Hash160,
    pub label: Option<String>,
    pub is_default: bool,
    pub lock: bool,
    pub scopes: SignerScopes,
    pub allowed_contracts: Vec<Hash160>,
    pub allowed_groups: Vec<Vec<u8>>,
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
        let hash = account.script_hash();
        let is_default = account.is_default();
        self.accounts.insert(hash, account);
        if is_default {
            self.set_default_internal(&hash)?;
        }
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
        let account = self
            .accounts
            .get(hash)
            .ok_or(WalletError::AccountNotFound)?;
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

    pub fn import_private_key(
        &mut self,
        private: PrivateKey,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let mut account = Account::from_private_key(private)?;
        account.set_default(make_default);
        let hash = account.script_hash();
        self.add_account(account)?;
        if make_default {
            self.set_default_internal(&hash)?;
        }
        Ok(hash)
    }

    pub fn import_nep2(
        &mut self,
        nep2: &str,
        passphrase: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let private = neo_crypto::nep2::decrypt_nep2(nep2, passphrase, address_version, scrypt)?;
        self.import_private_key(private, make_default)
    }

    pub fn export_nep2(
        &self,
        hash: &Hash160,
        passphrase: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
    ) -> Result<String, WalletError> {
        let account = self
            .accounts
            .get(hash)
            .ok_or(WalletError::AccountNotFound)?;
        let private = account.signer_key().ok_or(WalletError::WatchOnly)?;
        encrypt_nep2(private, passphrase, address_version, scrypt).map_err(Into::into)
    }

    pub fn import_wif(&mut self, wif: &str, make_default: bool) -> Result<Hash160, WalletError> {
        let decoded = wif
            .wif_decode(33)
            .map_err(|err| WalletError::InvalidWif(err.to_string()))?;
        if decoded.version() != 0x80 {
            return Err(WalletError::InvalidWif("unsupported WIF version".into()));
        }
        let data = decoded.data();
        if data.len() != 32 {
            return Err(WalletError::InvalidWif("invalid private key length".into()));
        }
        let mut buf = [0u8; 32];
        buf.copy_from_slice(data);
        let private = PrivateKey::from_slice(&buf)
            .map_err(|_| WalletError::InvalidWif("invalid private key".into()))?;
        self.import_private_key(private, make_default)
    }

    pub fn export_wif(&self, hash: &Hash160) -> Result<String, WalletError> {
        let account = self
            .accounts
            .get(hash)
            .ok_or(WalletError::AccountNotFound)?;
        let private = account.signer_key().ok_or(WalletError::WatchOnly)?;
        Ok(private.as_be_bytes().wif_encode(0x80, true))
    }

    pub fn add_watch_only(
        &mut self,
        script_hash: Hash160,
        contract: Option<Contract>,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let mut account = Account::watch_only_from_script(script_hash, contract);
        account.set_default(make_default);
        let hash = account.script_hash();
        self.add_account(account)?;
        if make_default {
            self.set_default_internal(&hash)?;
        }
        Ok(hash)
    }

    pub fn mark_default(&mut self, hash: &Hash160) -> Result<(), WalletError> {
        self.set_default_internal(hash)
    }

    pub fn set_lock(&mut self, hash: &Hash160, lock: bool) -> Result<(), WalletError> {
        let account = self
            .accounts
            .get_mut(hash)
            .ok_or(WalletError::AccountNotFound)?;
        account.set_lock(lock);
        Ok(())
    }

    pub fn set_label(&mut self, hash: &Hash160, label: Option<String>) -> Result<(), WalletError> {
        let account = self
            .accounts
            .get_mut(hash)
            .ok_or(WalletError::AccountNotFound)?;
        match label {
            Some(label) => account.set_label(label),
            None => account.clear_label(),
        }
        Ok(())
    }

    fn set_default_internal(&mut self, target: &Hash160) -> Result<(), WalletError> {
        if !self.accounts.contains_key(target) {
            return Err(WalletError::AccountNotFound);
        }
        for (hash, account) in self.accounts.iter_mut() {
            account.set_default(hash == target);
        }
        Ok(())
    }

    pub fn to_nep6_wallet(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
        password: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
    ) -> Result<Nep6Wallet, WalletError> {
        let mut accounts = Vec::with_capacity(self.accounts.len());
        for account in self.accounts.values() {
            let encrypted_key = match account.signer_key() {
                Some(private) => Some(encrypt_nep2(private, password, address_version, scrypt)?),
                None => None,
            };
            accounts.push(account.to_nep6_account(address_version, encrypted_key)?);
        }

        Ok(Nep6Wallet {
            name: name.into(),
            version: version.into(),
            scrypt: scrypt.into(),
            accounts,
            extra: None,
        })
    }

    pub fn from_nep6_wallet(
        nep6: &Nep6Wallet,
        password: Option<&str>,
        address_version: AddressVersion,
    ) -> Result<Self, WalletError> {
        let mut wallet = Wallet::new();
        for account in &nep6.accounts {
            let imported =
                Account::from_nep6_account(account, address_version, nep6.scrypt, password)?;
            wallet.add_account(imported)?;
        }
        Ok(wallet)
    }

    pub fn account_details(&self) -> Vec<AccountDetails> {
        self.accounts
            .values()
            .map(|account| AccountDetails {
                script_hash: account.script_hash(),
                label: account.label().map(|v| v.to_string()),
                is_default: account.is_default(),
                lock: account.is_locked(),
                scopes: account.signer_scopes(),
                allowed_contracts: account.allowed_contracts().to_vec(),
                allowed_groups: account.allowed_groups().to_vec(),
            })
            .collect()
    }
}

#[cfg(feature = "std")]
const SIGNER_METADATA_SUFFIX: &[u8] = b":signer_metadata";

#[cfg(feature = "std")]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredSignerMetadata {
    scopes: u8,
    allowed_contracts: Vec<Hash160>,
    allowed_groups: Vec<Vec<u8>>,
}

#[cfg(feature = "std")]
impl StoredSignerMetadata {
    fn from_account(account: &Account) -> Self {
        Self {
            scopes: account.signer_scopes().bits(),
            allowed_contracts: account.allowed_contracts().to_vec(),
            allowed_groups: account.allowed_groups().to_vec(),
        }
    }

    fn scopes(&self) -> SignerScopes {
        SignerScopes::from_bits_truncate(self.scopes)
    }

    fn is_default(&self) -> bool {
        self.scopes == SignerScopes::CALLED_BY_ENTRY.bits()
            && self.allowed_contracts.is_empty()
            && self.allowed_groups.is_empty()
    }
}

#[cfg(feature = "std")]
fn signer_metadata_storage_key(base: &[u8]) -> Vec<u8> {
    let mut key = base.to_vec();
    key.extend_from_slice(SIGNER_METADATA_SUFFIX);
    key
}

#[cfg(feature = "std")]
fn load_signer_metadata<S: Store + ?Sized>(
    store: &S,
    column: ColumnId,
    key: &[u8],
) -> Result<BTreeMap<Hash160, StoredSignerMetadata>, WalletError> {
    let storage_key = signer_metadata_storage_key(key);
    let raw = store
        .get(column, storage_key.as_slice())
        .map_err(|err| WalletError::Storage(err.to_string()))?;
    if let Some(bytes) = raw {
        serde_json::from_slice(&bytes)
            .map_err(|_| WalletError::Serialization("wallet signer metadata".into()))
    } else {
        Ok(BTreeMap::new())
    }
}

#[cfg(feature = "std")]
fn persist_signer_metadata<S: Store + ?Sized>(
    store: &S,
    column: ColumnId,
    key: &[u8],
    metadata: &BTreeMap<Hash160, StoredSignerMetadata>,
) -> Result<(), WalletError> {
    let storage_key = signer_metadata_storage_key(key);
    let json = serde_json::to_vec(metadata)
        .map_err(|_| WalletError::Serialization("wallet signer metadata".into()))?;
    store
        .put(column, storage_key, json)
        .map_err(|err| WalletError::Storage(err.to_string()))
}

#[cfg(feature = "std")]
pub struct WalletStorage<S: Store + ?Sized> {
    store: Arc<S>,
    column: ColumnId,
    key: Vec<u8>,
    keystore: Keystore,
    signer_metadata: BTreeMap<Hash160, StoredSignerMetadata>,
}

#[cfg(feature = "std")]
impl<S: Store + ?Sized> WalletStorage<S> {
    pub fn open(store: Arc<S>, column: ColumnId, key: Vec<u8>) -> Result<Self, WalletError> {
        let keystore = load_keystore(store.as_ref(), column, key.as_slice())?.unwrap_or_default();
        let mut signer_metadata = load_signer_metadata(store.as_ref(), column, key.as_slice())?;
        if keystore.entries.is_empty() {
            signer_metadata.clear();
        } else {
            signer_metadata.retain(|hash, _| {
                keystore
                    .entries
                    .iter()
                    .any(|entry| entry.script_hash == *hash)
            });
        }
        Ok(Self {
            store,
            column,
            key,
            keystore,
            signer_metadata,
        })
    }

    pub fn accounts(&self, password: &str) -> Result<Vec<Account>, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        let mut accounts: Vec<Account> = wallet.accounts.values().cloned().collect();
        for account in &mut accounts {
            if let Some(metadata) = self.signer_metadata.get(&account.script_hash()) {
                account.set_signer_scopes(metadata.scopes());
                account.set_allowed_contracts(metadata.allowed_contracts.clone());
                account.set_allowed_groups(metadata.allowed_groups.clone());
            }
        }
        Ok(accounts)
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
        let hash = account.script_hash();
        wallet.add_account(account.clone())?;
        self.signer_metadata.remove(&hash);
        self.store_wallet(wallet, password)?;
        Ok(account)
    }

    pub fn remove_account(&mut self, hash: &Hash160, password: &str) -> Result<(), WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        wallet.remove_account(hash)?;
        self.signer_metadata.remove(hash);
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

    pub fn import_wif(
        &mut self,
        wif: &str,
        password: &str,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        let hash = wallet.import_wif(wif, make_default)?;
        self.signer_metadata.remove(&hash);
        self.store_wallet(wallet, password)?;
        Ok(hash)
    }

    pub fn import_nep2(
        &mut self,
        nep2: &str,
        passphrase: &str,
        wallet_password: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, wallet_password)?;
        let hash = wallet.import_nep2(nep2, passphrase, scrypt, address_version, make_default)?;
        self.signer_metadata.remove(&hash);
        self.store_wallet(wallet, wallet_password)?;
        Ok(hash)
    }

    pub fn export_wif(&self, hash: &Hash160, password: &str) -> Result<String, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        wallet.export_wif(hash)
    }

    pub fn export_nep2(
        &self,
        hash: &Hash160,
        wallet_password: &str,
        passphrase: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
    ) -> Result<String, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, wallet_password)?;
        wallet.export_nep2(hash, passphrase, scrypt, address_version)
    }

    pub fn account_details(&self, password: &str) -> Result<Vec<AccountDetails>, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        let mut details = wallet.account_details();
        for detail in &mut details {
            if let Some(metadata) = self.signer_metadata.get(&detail.script_hash) {
                detail.scopes = metadata.scopes();
                detail.allowed_contracts = metadata.allowed_contracts.clone();
                detail.allowed_groups = metadata.allowed_groups.clone();
            }
        }
        Ok(details)
    }

    pub fn update_signer_metadata(
        &mut self,
        hash: &Hash160,
        password: &str,
        scopes: SignerScopes,
        allowed_contracts: Vec<Hash160>,
        allowed_groups: Vec<Vec<u8>>,
    ) -> Result<(), WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        let account = wallet
            .accounts
            .get_mut(hash)
            .ok_or(WalletError::AccountNotFound)?;
        account
            .update_signer_metadata(scopes, allowed_contracts, allowed_groups)?;
        let metadata = StoredSignerMetadata::from_account(account);
        if metadata.is_default() {
            self.signer_metadata.remove(hash);
        } else {
            self.signer_metadata.insert(*hash, metadata);
        }
        self.store_wallet(wallet, password)
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
        persist_signer_metadata(
            self.store.as_ref(),
            self.column,
            self.key.as_slice(),
            &self.signer_metadata,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SignerScopes;
    use hex_literal::hex;
    use neo_base::{hash::Hash160, AddressVersion};
    use neo_crypto::{ecc256::PrivateKey, scrypt::ScryptParams};

    const NEP2_VECTOR: &str = "6PYRzCDe46gkaR1E9AX3GyhLgQehypFvLG2KknbYjeNHQ3MZR2iqg8mcN3";
    const NEP2_PASSWORD: &str = "Satoshi";
    const WIF_VECTOR: &str = "L3tgppXLgdaeqSGSFw1Go3skBiy8vQAM7YMXvTHsKQtE16PBncSU";

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
            .to_nep6_wallet("name", "1.0", NEP2_PASSWORD, scrypt, address_version)
            .unwrap();

        assert_eq!(nep6.accounts.len(), 1);
        let account_entry = &nep6.accounts[0];
        assert_eq!(account_entry.key.as_deref(), Some(NEP2_VECTOR));
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
            Wallet::from_nep6_wallet(&nep6, Some(NEP2_PASSWORD), address_version).unwrap();
        assert_eq!(restored.len(), 1);
        let restored_account = restored.account(&hash).unwrap();
        assert!(restored_account
            .signer_scopes()
            .contains(SignerScopes::CUSTOM_CONTRACTS));
        assert_eq!(restored_account.allowed_contracts(), &[contract_hash]);
        assert_eq!(restored_account.allowed_groups().len(), 1);
    }

    #[test]
    fn wallet_import_export_wif() {
        let mut wallet = Wallet::new();
        let hash = wallet.import_wif(WIF_VECTOR, true).unwrap();
        let exported = wallet.export_wif(&hash).unwrap();
        assert_eq!(exported, WIF_VECTOR);
        assert!(wallet.account(&hash).unwrap().is_default());
    }

    #[test]
    fn wallet_import_export_nep2() {
        let mut wallet = Wallet::new();
        let scrypt = ScryptParams { n: 2, r: 1, p: 1 };
        let hash = wallet
            .import_nep2(
                NEP2_VECTOR,
                NEP2_PASSWORD,
                scrypt,
                AddressVersion::MAINNET,
                true,
            )
            .unwrap();

        let exported = wallet
            .export_nep2(&hash, NEP2_PASSWORD, scrypt, AddressVersion::MAINNET)
            .unwrap();
        assert_eq!(exported, NEP2_VECTOR);
    }

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

    #[cfg(feature = "std")]
    #[test]
    fn wallet_storage_persists_signer_metadata() {
        use neo_store::{ColumnId, MemoryStore};

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
        let contract =
            Hash160::from_slice(&hex!("17b24dbdc30b30f33d05a281a81f0c0a5f94b8c0")).unwrap();
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

        // Resetting metadata to default should remove the persisted record.
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

        let reset =
            WalletStorage::open(store, column, key).expect("reload after reset");
        let details = reset
            .account_details("pass")
            .expect("details after reset");
        assert_eq!(details.len(), 1);
        let detail = &details[0];
        assert_eq!(detail.scopes, SignerScopes::CALLED_BY_ENTRY);
        assert!(detail.allowed_contracts.is_empty());
        assert!(detail.allowed_groups.is_empty());
    }

}
