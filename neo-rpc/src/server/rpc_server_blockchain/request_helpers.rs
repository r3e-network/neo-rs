use std::str::FromStr;

use neo_execution::contract_state::ContractState;
use neo_payloads::block::Block;
use neo_primitives::{UInt160, UInt256};
use serde_json::Value;

use crate::server::contract_state_provider::{
    DeployedContractProvider, DeployedContractProviderFactory,
    NativeDeployedContractProviderFactory,
};
use crate::server::ledger_queries;
use crate::server::model::block_hash_or_index::BlockHashOrIndex as RpcBlockHashOrIndex;
use crate::server::model::contract_name_or_hash_or_id::ContractNameOrHashOrId;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_messages, expect_u32_param_with_message,
    expect_uint256_param_with_message, internal_error,
};

use super::RpcServerBlockchain;

pub(super) use crate::server::rpc_helpers::NoParamsRequest;

pub(super) struct BlockHeightRequest {
    pub(super) height: u32,
}

impl BlockHeightRequest {
    pub(super) fn parse(params: &[Value], method: &str) -> Result<Self, RpcException> {
        Ok(Self {
            height: RpcServerBlockchain::expect_u32_param(params, 0, method)?,
        })
    }
}

pub(super) struct BlockPayloadRequest {
    pub(super) identifier: RpcBlockHashOrIndex,
    pub(super) verbose: bool,
}

impl BlockPayloadRequest {
    pub(super) fn parse(params: &[Value], method: &str) -> Result<Self, RpcException> {
        Ok(Self {
            identifier: RpcServerBlockchain::parse_block_identifier(params, method)?,
            verbose: RpcServerBlockchain::parse_verbose(params.get(1))?,
        })
    }
}

pub(super) struct GetContractStateRequest {
    pub(super) identifier: ContractNameOrHashOrId,
}

impl GetContractStateRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            identifier: RpcServerBlockchain::parse_contract_identifier(params, "getcontractstate")?,
        })
    }
}

pub(super) struct GetStorageRequest {
    pub(super) identifier: ContractNameOrHashOrId,
    pub(super) key_bytes: Vec<u8>,
}

impl GetStorageRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            identifier: RpcServerBlockchain::parse_contract_identifier(params, "getstorage")?,
            key_bytes: expect_base64_param_with_messages(
                params,
                1,
                "getstorage requires Base64 key parameter",
                |key| format!("invalid Base64 storage key: {key}"),
            )?,
        })
    }
}

pub(super) struct FindStorageRequest {
    pub(super) identifier: ContractNameOrHashOrId,
    pub(super) prefix_bytes: Vec<u8>,
    pub(super) start: usize,
}

impl FindStorageRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            identifier: RpcServerBlockchain::parse_contract_identifier(params, "findstorage")?,
            prefix_bytes: expect_base64_param_with_messages(
                params,
                1,
                "findstorage requires Base64 prefix parameter",
                |prefix| format!("invalid Base64 storage prefix: {prefix}"),
            )?,
            start: parse_find_storage_start(params)?,
        })
    }
}

fn parse_find_storage_start(params: &[Value]) -> Result<usize, RpcException> {
    match params.get(2) {
        None => Ok(0),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(non_negative_start_error),
        _ => Err(non_negative_start_error()),
    }
}

fn non_negative_start_error() -> RpcException {
    RpcException::from(
        RpcError::invalid_params().with_data("start index must be a non-negative integer"),
    )
}

pub(super) struct RawMemPoolRequest {
    pub(super) include_unverified: bool,
}

impl RawMemPoolRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            include_unverified: parse_should_get_unverified(params.first())?,
        })
    }
}

fn parse_should_get_unverified(value: Option<&Value>) -> Result<bool, RpcException> {
    match value {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(Value::Number(number)) => match number.as_u64() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(should_get_unverified_error()),
        },
        _ => Err(should_get_unverified_error()),
    }
}

fn should_get_unverified_error() -> RpcException {
    RpcException::from(
        RpcError::invalid_params().with_data("shouldGetUnverified must be a boolean"),
    )
}

pub(super) struct RawTransactionRequest {
    pub(super) hash: UInt256,
    pub(super) verbose: bool,
}

impl RawTransactionRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            hash: RpcServerBlockchain::expect_hash_param(params, 0, "getrawtransaction")?,
            verbose: RpcServerBlockchain::parse_verbose(params.get(1))?,
        })
    }
}

pub(super) struct TransactionHeightRequest {
    pub(super) hash: UInt256,
}

impl TransactionHeightRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            hash: RpcServerBlockchain::expect_hash_param(params, 0, "gettransactionheight")?,
        })
    }
}

impl RpcServerBlockchain {
    pub(super) fn parse_block_identifier(
        params: &[Value],
        method: &str,
    ) -> Result<RpcBlockHashOrIndex, RpcException> {
        let token = params.first().ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{method} requires at least one parameter")),
            )
        })?;

        match token {
            Value::Number(number) => {
                let value = number
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| {
                        RpcException::from(
                            RpcError::invalid_params()
                                .with_data(format!("{method} index is out of range")),
                        )
                    })?;
                Ok(RpcBlockHashOrIndex::from_index(value))
            }
            Value::String(text) => RpcBlockHashOrIndex::try_parse(text).ok_or_else(|| {
                RpcException::from(RpcError::invalid_params().with_data(format!(
                    "{method} expects block hash or index, got '{text}'"
                )))
            }),
            _ => Err(RpcException::from(RpcError::invalid_params().with_data(
                format!("{method} expects the first parameter to be hash or index"),
            ))),
        }
    }

    pub(super) fn parse_verbose(arg: Option<&Value>) -> Result<bool, RpcException> {
        match arg {
            None => Ok(false),
            Some(Value::Bool(value)) => Ok(*value),
            Some(Value::Number(number)) => match number.as_u64() {
                Some(0) => Ok(false),
                Some(1) => Ok(true),
                _ => Err(RpcException::from(
                    RpcError::invalid_params().with_data("verbose flag must be a boolean or 0/1"),
                )),
            },
            _ => Err(RpcException::from(
                RpcError::invalid_params().with_data("verbose flag must be a boolean"),
            )),
        }
    }

    pub(super) fn fetch_payload_block(
        store: &neo_storage::persistence::StoreCache,
        identifier: &RpcBlockHashOrIndex,
    ) -> Result<Block, RpcException> {
        ledger_queries::get_full_block(store.data_cache(), identifier)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_block()))
    }

    pub(super) fn expect_u32_param(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<u32, RpcException> {
        expect_u32_param_with_message(
            params,
            index,
            format!("{} expects numeric parameter {}", method, index + 1),
        )
    }

    pub(super) fn expect_hash_param(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<UInt256, RpcException> {
        expect_uint256_param_with_message(
            params,
            index,
            format!("{} expects string parameter {}", method, index + 1),
            "hash",
        )
    }

    pub(super) fn parse_contract_identifier(
        params: &[Value],
        method: &str,
    ) -> Result<ContractNameOrHashOrId, RpcException> {
        let token = params.first().ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{method} requires at least one parameter")),
            )
        })?;

        match token {
            Value::Number(number) => {
                let value = number
                    .as_i64()
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| {
                        RpcException::from(
                            RpcError::invalid_params()
                                .with_data(format!("{method} contract id out of range")),
                        )
                    })?;
                Ok(ContractNameOrHashOrId::from_id(value))
            }
            Value::String(text) => ContractNameOrHashOrId::try_parse(text).ok_or_else(|| {
                RpcException::from(
                    RpcError::invalid_params()
                        .with_data(format!("invalid contract identifier '{text}'")),
                )
            }),
            _ => Err(RpcException::from(RpcError::invalid_params().with_data(
                format!("{method} expects contract identifier as string or integer"),
            ))),
        }
    }

    pub(super) fn load_contract_state(
        store: &neo_storage::persistence::StoreCache,
        identifier: &ContractNameOrHashOrId,
    ) -> Result<Option<ContractState>, RpcException> {
        let provider = NativeDeployedContractProviderFactory.provider();
        let snapshot = store.data_cache();
        match identifier {
            ContractNameOrHashOrId::Id(id) => provider
                .contract_state_by_id(snapshot, *id)
                .map_err(internal_error),
            ContractNameOrHashOrId::Hash(hash) => provider
                .contract_state_by_hash(snapshot, hash)
                .map_err(internal_error),
            ContractNameOrHashOrId::Name(name) => {
                let hash = Self::contract_name_to_hash(name)?;
                provider
                    .contract_state_by_hash(snapshot, &hash)
                    .map_err(internal_error)
            }
        }
    }

    pub(super) fn resolve_contract_id(
        store: &neo_storage::persistence::StoreCache,
        identifier: &ContractNameOrHashOrId,
    ) -> Result<i32, RpcException> {
        if let ContractNameOrHashOrId::Id(id) = identifier {
            let state = NativeDeployedContractProviderFactory
                .provider()
                .contract_state_by_id(store.data_cache(), *id)
                .map_err(internal_error)?;
            state
                .map(|contract| contract.id)
                .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))
        } else {
            let contract = Self::load_contract_state(store, identifier)?
                .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;
            Ok(contract.id)
        }
    }

    fn contract_name_to_hash(name: &str) -> Result<UInt160, RpcException> {
        let registry = crate::server::native_queries::NativeQueries::native_registry();
        if let Some(contract) = registry.get_by_name(name) {
            return Ok(contract.hash());
        }
        UInt160::from_str(name).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("invalid contract identifier '{name}': {err}")),
            )
        })
    }
}
