use neo_primitives::UInt256;

use crate::server::rpc_error::RpcError;

/// RPC parameter that can refer to a block by height or hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockHashOrIndex {
    /// Block height.
    Index(u32),
    /// Block hash.
    Hash(UInt256),
}

impl BlockHashOrIndex {
    /// Construct a block reference from a height.
    #[must_use]
    pub const fn from_index(index: u32) -> Self {
        Self::Index(index)
    }

    /// Construct a block reference from a hash.
    #[must_use]
    pub const fn from_hash(hash: UInt256) -> Self {
        Self::Hash(hash)
    }

    /// Return whether this reference is a block height.
    #[must_use]
    pub const fn is_index(&self) -> bool {
        matches!(self, Self::Index(_))
    }

    /// Parse a block height or UInt256 hash from an RPC string parameter.
    #[must_use]
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

    /// Return the block height or an RPC invalid-params error.
    pub fn as_index(&self) -> Result<u32, RpcError> {
        match self {
            Self::Index(index) => Ok(*index),
            other => Err(RpcError::invalid_params()
                .with_data(format!("Value {other:?} is not a valid block index"))),
        }
    }

    /// Return the block hash or an RPC invalid-params error.
    pub fn as_hash(&self) -> Result<UInt256, RpcError> {
        match self {
            Self::Hash(hash) => Ok(*hash),
            other => Err(RpcError::invalid_params()
                .with_data(format!("Value {other:?} is not a valid block hash"))),
        }
    }
}
