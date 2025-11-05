use neo_base::{hash::Hash256, Bytes, NeoDecodeDerive, NeoEncodeDerive};
use serde::{Deserialize, Serialize};

use crate::traits::{Column, ColumnId};

pub struct Headers;
pub struct Blocks;

impl Column for Headers {
    const ID: ColumnId = ColumnId::new("headers");
}

impl Column for Blocks {
    const ID: ColumnId = ColumnId::new("blocks");
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, NeoEncodeDerive, NeoDecodeDerive, Serialize, Deserialize,
)]
pub struct HeightKey {
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, NeoEncodeDerive, NeoDecodeDerive, Serialize, Deserialize)]
pub struct HeaderRecord {
    pub hash: Hash256,
    pub height: u32,
    pub raw: Bytes,
}

impl HeaderRecord {
    pub fn key(&self) -> HeightKey {
        HeightKey {
            height: self.height,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, NeoEncodeDerive, NeoDecodeDerive, Serialize, Deserialize)]
pub struct HashKey {
    pub hash: Hash256,
}

#[derive(Clone, Debug, PartialEq, Eq, NeoEncodeDerive, NeoDecodeDerive, Serialize, Deserialize)]
pub struct BlockRecord {
    pub hash: Hash256,
    pub raw: Bytes,
}

impl BlockRecord {
    pub fn key(&self) -> HashKey {
        HashKey { hash: self.hash }
    }
}
