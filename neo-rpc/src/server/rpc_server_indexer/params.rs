use neo_indexer::IndexerService;
use neo_primitives::{UInt160, UInt256};
use serde_json::Value;

use super::RpcServerIndexer;
use super::support::{BlockSelector, PageBounds};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_script_hash_or_address_param, expect_uint256_param_with_message, invalid_params,
    optional_usize_param, parse_uint256_text_with_label,
};

pub(super) use crate::server::rpc_helpers::NoParamsRequest;

pub(super) struct BlockIndexRequest {
    pub(super) selector: BlockSelector,
}

impl BlockIndexRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        RpcServerIndexer::expect_exact_params(params, 1, "getblockindex")?;
        Ok(Self {
            selector: RpcServerIndexer::parse_block_selector(params, "getblockindex")?,
        })
    }
}

pub(super) struct TransactionIndexRequest {
    pub(super) hash: UInt256,
}

impl TransactionIndexRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        RpcServerIndexer::expect_exact_params(params, 1, "gettransactionindex")?;
        Ok(Self {
            hash: RpcServerIndexer::expect_uint256(params, 0, "gettransactionindex")?,
        })
    }
}

pub(super) struct TransactionPageRequest {
    pub(super) hash: UInt256,
    pub(super) page: PageRequest,
}

impl TransactionPageRequest {
    pub(super) fn parse(
        params: &[Value],
        bounds: PageBounds,
        method: &str,
    ) -> Result<Self, RpcException> {
        Ok(Self {
            hash: RpcServerIndexer::expect_uint256(params, 0, method)?,
            page: PageRequest::parse(params, 1, bounds, method)?,
        })
    }
}

pub(super) struct BlockPageRequest {
    pub(super) selector: BlockSelector,
    pub(super) page: PageRequest,
}

impl BlockPageRequest {
    pub(super) fn parse(
        params: &[Value],
        bounds: PageBounds,
        method: &str,
    ) -> Result<Self, RpcException> {
        Ok(Self {
            selector: RpcServerIndexer::parse_block_selector(params, method)?,
            page: PageRequest::parse(params, 1, bounds, method)?,
        })
    }
}

pub(super) struct AccountPageRequest {
    pub(super) account: UInt160,
    pub(super) page: PageRequest,
}

impl AccountPageRequest {
    pub(super) fn parse(
        params: &[Value],
        address_version: u8,
        bounds: PageBounds,
        method: &str,
    ) -> Result<Self, RpcException> {
        Ok(Self {
            account: RpcServerIndexer::expect_account(params, 0, method, address_version)?,
            page: PageRequest::parse(params, 1, bounds, method)?,
        })
    }
}

pub(super) struct ContractActivityRequest {
    pub(super) contract_hash: UInt160,
    pub(super) event_name: Option<String>,
    pub(super) page: PageRequest,
}

impl ContractActivityRequest {
    pub(super) fn parse(
        params: &[Value],
        address_version: u8,
        bounds: PageBounds,
        method: &str,
    ) -> Result<Self, RpcException> {
        let (contract_hash, event_name, skip, limit) =
            RpcServerIndexer::parse_contract_activity_params(
                params,
                address_version,
                method,
                bounds,
            )?;
        Ok(Self {
            contract_hash,
            event_name,
            page: PageRequest { skip, limit },
        })
    }
}

pub(super) struct PageRequest {
    pub(super) skip: usize,
    pub(super) limit: usize,
}

impl PageRequest {
    pub(super) fn parse(
        params: &[Value],
        skip_index: usize,
        bounds: PageBounds,
        method: &str,
    ) -> Result<Self, RpcException> {
        let (skip, limit) = RpcServerIndexer::parse_page(params, skip_index, bounds, method)?;
        Ok(Self { skip, limit })
    }
}

impl RpcServerIndexer {
    pub(super) fn expect_exact_params(
        params: &[Value],
        expected: usize,
        method: &str,
    ) -> Result<(), RpcException> {
        if params.len() == expected {
            Ok(())
        } else {
            Err(invalid_params(format!(
                "{method} expects exactly {expected} {}",
                Self::parameter_word(expected)
            )))
        }
    }

    fn expect_max_params(params: &[Value], max: usize, method: &str) -> Result<(), RpcException> {
        if params.len() <= max {
            Ok(())
        } else {
            Err(invalid_params(format!(
                "{method} expects at most {max} {}",
                Self::parameter_word(max)
            )))
        }
    }

    fn parameter_word(count: usize) -> &'static str {
        if count == 1 {
            "parameter"
        } else {
            "parameters"
        }
    }

    pub(super) fn parse_block_selector(
        params: &[Value],
        method: &str,
    ) -> Result<BlockSelector, RpcException> {
        let value = params
            .first()
            .ok_or_else(|| invalid_params(format!("{method} expects hash or height parameter")))?;
        match value {
            Value::Number(number) => number
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .map(BlockSelector::Height)
                .ok_or_else(|| invalid_params(format!("{method} expects u32 height"))),
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.len() <= 10 {
                    if let Ok(height) = trimmed.parse::<u32>() {
                        return Ok(BlockSelector::Height(height));
                    }
                }
                parse_uint256_text_with_label(trimmed, "block hash").map(BlockSelector::Hash)
            }
            _ => Err(invalid_params(format!(
                "{method} expects hash string or height integer"
            ))),
        }
    }

    pub(super) fn block_hash_from_selector_value(
        service: &IndexerService,
        selector: BlockSelector,
    ) -> Result<Option<UInt256>, RpcException> {
        Ok(match selector {
            BlockSelector::Height(height) => service
                .try_block_by_height(height)
                .map_err(Self::indexer_error)?
                .map(|record| record.hash),
            BlockSelector::Hash(hash) => Some(hash),
        })
    }

    pub(super) fn expect_uint256(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<UInt256, RpcException> {
        expect_uint256_param_with_message(
            params,
            index,
            format!("{method} expects hash parameter"),
            "hash",
        )
    }

    pub(super) fn expect_account(
        params: &[Value],
        index: usize,
        method: &str,
        address_version: u8,
    ) -> Result<UInt160, RpcException> {
        expect_script_hash_or_address_param(params, index, method, address_version)
    }

    pub(super) fn parse_page(
        params: &[Value],
        skip_index: usize,
        bounds: PageBounds,
        method: &str,
    ) -> Result<(usize, usize), RpcException> {
        Self::expect_max_params(params, skip_index + 2, method)?;
        let message = || format!("{method} expects unsigned integer");
        let skip = optional_usize_param(params.get(skip_index), 0, message())?;
        let limit =
            optional_usize_param(params.get(skip_index + 1), bounds.default_limit, message())?
                .min(bounds.max_limit);
        Ok((skip, limit))
    }

    pub(super) fn parse_contract_activity_params(
        params: &[Value],
        address_version: u8,
        method: &str,
        bounds: PageBounds,
    ) -> Result<(UInt160, Option<String>, usize, usize), RpcException> {
        let contract_hash = Self::expect_account(params, 0, method, address_version)?;
        let (event_name, page_offset) = match params.get(1) {
            Some(Value::String(text)) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    (None, 2)
                } else {
                    (Some(trimmed.to_string()), 2)
                }
            }
            Some(Value::Null) => (None, 2),
            _ => (None, 1),
        };
        let (skip, limit) = Self::parse_page(params, page_offset, bounds, method)?;
        Ok((contract_hash, event_name, skip, limit))
    }
}
