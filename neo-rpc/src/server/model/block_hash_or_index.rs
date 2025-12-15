use neo_core::UInt256;

use crate::server::rpc_error::RpcError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockHashOrIndex {
    Index(u32),
    Hash(UInt256),
}

impl BlockHashOrIndex {
    pub fn from_index(index: u32) -> Self {
        Self::Index(index)
    }

    pub fn from_hash(hash: UInt256) -> Self {
        Self::Hash(hash)
    }

    pub fn is_index(&self) -> bool {
        matches!(self, Self::Index(_))
    }

    pub fn try_parse(value: &str) -> Option<Self> {
        if let Ok(index) = value.parse::<u32>() {
            return Some(Self::Index(index));
        }

        let mut result = None;
        if UInt256::try_parse(value, &mut result) {
            if let Some(hash) = result {
                return Some(Self::Hash(hash));
            }
        }

        None
    }

    pub fn as_index(&self) -> Result<u32, RpcError> {
        match self {
            Self::Index(index) => Ok(*index),
            other => Err(RpcError::invalid_params()
                .with_data(format!("Value {other:?} is not a valid block index"))),
        }
    }

    pub fn as_hash(&self) -> Result<UInt256, RpcError> {
        match self {
            Self::Hash(hash) => Ok(*hash),
            other => Err(RpcError::invalid_params()
                .with_data(format!("Value {other:?} is not a valid block hash"))),
        }
    }
}
