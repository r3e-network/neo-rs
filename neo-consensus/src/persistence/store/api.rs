use neo_store::{Column, Store, StoreExt};

use crate::{state::ConsensusState, DbftEngine, SnapshotState, ValidatorSet};

use super::{
    super::{ConsensusColumn, SnapshotKey},
    error::PersistenceError,
};

pub fn persist_engine<S: Store + ?Sized>(
    store: &S,
    key: SnapshotKey,
    engine: &DbftEngine,
) -> Result<(), PersistenceError> {
    let snapshot = engine.snapshot();
    store
        .put_encoded(ConsensusColumn::ID, &key, &snapshot)
        .map_err(PersistenceError::from)
}

pub fn load_engine<S: Store + ?Sized>(
    store: &S,
    validators: ValidatorSet,
    key: SnapshotKey,
) -> Result<Option<DbftEngine>, PersistenceError> {
    match store.get_decoded::<SnapshotKey, SnapshotState>(ConsensusColumn::ID, &key)? {
        Some(snapshot) => {
            let state = ConsensusState::from_snapshot(validators, snapshot)?;
            Ok(Some(DbftEngine::new(state)))
        }
        None => Ok(None),
    }
}

pub fn clear_snapshot<S: Store + ?Sized>(
    store: &S,
    key: SnapshotKey,
) -> Result<(), PersistenceError> {
    store
        .delete_encoded::<SnapshotKey>(ConsensusColumn::ID, &key)
        .map_err(PersistenceError::from)
}
