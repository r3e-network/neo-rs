use alloc::{collections::BTreeMap, vec::Vec};

use neo_base::hash::Hash160;
use neo_store::{ColumnId, Store};
use serde::{Deserialize, Serialize};

use crate::{account::Account, error::WalletError, signer::SignerScopes};

const SIGNER_METADATA_SUFFIX: &[u8] = b":signer_metadata";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StoredSignerMetadata {
    pub scopes: u8,
    pub allowed_contracts: Vec<Hash160>,
    pub allowed_groups: Vec<Vec<u8>>,
}

impl StoredSignerMetadata {
    pub fn from_account(account: &Account) -> Self {
        Self {
            scopes: account.signer_scopes().bits(),
            allowed_contracts: account.allowed_contracts().to_vec(),
            allowed_groups: account.allowed_groups().to_vec(),
        }
    }

    pub fn scopes(&self) -> SignerScopes {
        SignerScopes::from_bits_truncate(self.scopes)
    }

    pub fn is_default(&self) -> bool {
        self.scopes == SignerScopes::CALLED_BY_ENTRY.bits()
            && self.allowed_contracts.is_empty()
            && self.allowed_groups.is_empty()
    }
}

fn signer_metadata_storage_key(base: &[u8]) -> Vec<u8> {
    let mut key = base.to_vec();
    key.extend_from_slice(SIGNER_METADATA_SUFFIX);
    key
}

pub fn load_signer_metadata<S: Store + ?Sized>(
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

pub fn persist_signer_metadata<S: Store + ?Sized>(
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
