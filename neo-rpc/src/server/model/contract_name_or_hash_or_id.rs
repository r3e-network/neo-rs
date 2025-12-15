use neo_core::UInt160;

use crate::server::rpc_error::RpcError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContractNameOrHashOrId {
    Id(i32),
    Hash(UInt160),
    Name(String),
}

impl ContractNameOrHashOrId {
    pub fn from_id(id: i32) -> Self {
        Self::Id(id)
    }

    pub fn from_hash(hash: UInt160) -> Self {
        Self::Hash(hash)
    }

    pub fn from_name(name: impl Into<String>) -> Self {
        Self::Name(name.into())
    }

    pub fn is_id(&self) -> bool {
        matches!(self, Self::Id(_))
    }

    pub fn is_hash(&self) -> bool {
        matches!(self, Self::Hash(_))
    }

    pub fn is_name(&self) -> bool {
        matches!(self, Self::Name(_))
    }

    pub fn try_parse(value: &str) -> Option<Self> {
        if let Ok(id) = value.parse::<i32>() {
            return Some(Self::Id(id));
        }

        let mut result = None;
        if UInt160::try_parse(value, &mut result) {
            if let Some(hash) = result {
                return Some(Self::Hash(hash));
            }
        }

        if !value.is_empty() {
            return Some(Self::Name(value.to_string()));
        }

        None
    }

    pub fn as_id(&self) -> Result<i32, RpcError> {
        match self {
            Self::Id(id) => Ok(*id),
            other => Err(RpcError::invalid_params()
                .with_data(format!("Value {other:?} is not a valid contract id"))),
        }
    }

    pub fn as_hash(&self) -> Result<UInt160, RpcError> {
        match self {
            Self::Hash(hash) => Ok(*hash),
            other => Err(RpcError::invalid_params()
                .with_data(format!("Value {other:?} is not a valid contract hash"))),
        }
    }

    pub fn as_name(&self) -> Result<&str, RpcError> {
        match self {
            Self::Name(name) => Ok(name.as_str()),
            other => Err(RpcError::invalid_params()
                .with_data(format!("Value {other:?} is not a valid contract name"))),
        }
    }
}
