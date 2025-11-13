use neo_base::{
    encoding::{NeoDecode, NeoEncode, SliceReader},
    hash::Hash160,
};
use neo_store::{ColumnId, Store, StoreError};

use crate::state::ContractState;

const CONTRACT_COLUMN_NAME: &str = "contract";

pub(crate) fn contract_column() -> ColumnId {
    ColumnId::new(CONTRACT_COLUMN_NAME)
}

pub(crate) fn load_contract_state<S: Store + ?Sized>(
    store: &S,
    hash: &Hash160,
) -> Result<Option<ContractState>, StoreError> {
    match store.get(contract_column(), hash.as_slice())? {
        Some(bytes) => {
            let mut reader = neo_base::encoding::SliceReader::new(bytes.as_slice());
            ContractState::neo_decode(&mut reader)
                .map(Some)
                .map_err(|err| StoreError::backend(format!("decode contract: {err}")))
        }
        None => Ok(None),
    }
}

pub(crate) fn put_contract_state<S: Store + ?Sized>(
    store: &S,
    state: &ContractState,
) -> Result<(), StoreError> {
    store.put(contract_column(), state.hash.to_vec(), state.to_vec())
}
