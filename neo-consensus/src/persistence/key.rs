use neo_store::{Column, ColumnId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug)]
pub struct ConsensusColumn;

impl Column for ConsensusColumn {
    const ID: ColumnId = ColumnId::new("consensus.snapshot");
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    neo_base::NeoEncodeDerive,
    neo_base::NeoDecodeDerive,
    Serialize,
    Deserialize,
)]
pub struct SnapshotKey {
    pub network: u32,
}
