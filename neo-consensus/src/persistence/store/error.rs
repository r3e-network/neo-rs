use crate::error::ConsensusError;

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("store error: {0}")]
    Store(#[from] neo_store::StoreError),

    #[error("invalid snapshot: {0}")]
    Snapshot(#[from] ConsensusError),
}
