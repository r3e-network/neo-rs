pub fn persist_keystore<S: Store + ?Sized>(
    store: &S,
    column: ColumnId,
    key: Vec<u8>,
    keystore: &Keystore,
) -> Result<(), WalletError> {
    let json = serde_json::to_vec(keystore).map_err(|_| WalletError::InvalidKeystore)?;
    store
        .put(column, key, json)
        .map_err(|err| WalletError::Storage(err.to_string()))
}

pub fn load_keystore<S: Store + ?Sized>(
    store: &S,
    column: ColumnId,
    key: &[u8],
) -> Result<Option<Keystore>, WalletError> {
    match store
        .get(column, key)
        .map_err(|err| WalletError::Storage(err.to_string()))?
    {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| WalletError::InvalidKeystore),
        None => Ok(None),
    }
}
use crate::{Keystore, WalletError};
use neo_store::{ColumnId, Store};
