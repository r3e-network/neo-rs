use alloc::sync::Arc;
use alloc::{collections::BTreeMap, vec::Vec};

use neo_base::{hash::Hash160, AddressVersion};
use neo_crypto::ecc256::PrivateKey;
use neo_crypto::{scrypt::ScryptParams, SignatureBytes};
use neo_store::{ColumnId, Store};

use crate::{
    account::Account,
    error::WalletError,
    keystore::{load_keystore, persist_keystore, Keystore},
    signer::SignerScopes,
};

use super::super::core::{AccountDetails, Wallet};
use super::metadata::{load_signer_metadata, persist_signer_metadata, StoredSignerMetadata};

mod accounts;
mod crypto;
mod metadata;

pub struct WalletStorage<S: Store + ?Sized> {
    store: Arc<S>,
    column: ColumnId,
    key: Vec<u8>,
    keystore: Keystore,
    signer_metadata: BTreeMap<Hash160, StoredSignerMetadata>,
}

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

    pub(super) fn store_wallet(
        &mut self,
        wallet: Wallet,
        password: &str,
    ) -> Result<(), WalletError> {
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
