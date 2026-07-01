//! # neo-rpc::server::rpc_server_indexer
//!
//! Indexer-backed RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `params`: RPC endpoint parameter records.
//! - `responses`: RPC response construction helpers.
//! - `status`: RPC status response records.
//! - `tests`: Module-local tests and regression coverage.

use std::sync::Arc;

use neo_indexer::{IndexerError, IndexerService};
use neo_primitives::UInt256;
use serde_json::Value;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::{RpcHandler, RpcServer};

mod params;
mod responses;
mod status;

const STANDARD_PAGE_BOUNDS: PageBounds = PageBounds {
    default_limit: 100,
    max_limit: 1_000,
};

#[derive(Debug, Clone, Copy)]
struct PageBounds {
    default_limit: usize,
    max_limit: usize,
}

/// RPC method group for the read-side Neo indexer service.
pub struct RpcServerIndexer;

impl RpcServerIndexer {
    /// Registers NeoIndexer RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getindexerstatus" => Self::get_indexer_status,
            "getblockindex" => Self::get_block_index,
            "getblockindexes" => Self::get_block_indexes,
            "gettransactionindex" => Self::get_transaction_index,
            "getblocktransactions" => Self::get_block_transactions,
            "getaddresstransactions" => Self::get_address_transactions,
            "getcontracttransactions" => Self::get_contract_transactions,
            "getaddressnotifications" => Self::get_address_notifications,
            "getblocknotifications" => Self::get_block_notifications,
            "gettransactionnotifications" => Self::get_transaction_notifications,
            "getcontractnotifications" => Self::get_contract_notifications,
        ]
    }

    fn get_indexer_status(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        Self::expect_no_params(params, "getindexerstatus")?;
        let service = Self::service(server)?;
        Self::indexer_status_json(server, &service).map_err(Self::indexer_error)
    }

    fn get_block_index(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        Self::expect_exact_params(params, 1, "getblockindex")?;
        let service = Self::service(server)?;
        let record = match Self::parse_block_selector(params, "getblockindex")? {
            BlockSelector::Height(height) => service
                .try_block_by_height(height)
                .map_err(Self::indexer_error)?,
            BlockSelector::Hash(hash) => service
                .try_block_by_hash(&hash)
                .map_err(Self::indexer_error)?,
        };
        Ok(record.map_or(Value::Null, Self::block_to_json))
    }

    fn get_block_indexes(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let (skip, limit) = Self::parse_page(params, 0, STANDARD_PAGE_BOUNDS, "getblockindexes")?;

        Ok(Value::Array(
            service
                .try_blocks(skip, limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(Self::block_to_json)
                .collect(),
        ))
    }

    fn get_transaction_index(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        Self::expect_exact_params(params, 1, "gettransactionindex")?;
        let service = Self::service(server)?;
        let hash = Self::expect_uint256(params, 0, "gettransactionindex")?;
        let address_version = server.system().settings().address_version;
        Ok(service
            .try_transaction(&hash)
            .map_err(Self::indexer_error)?
            .map(|record| Self::transaction_to_json(&record, address_version))
            .unwrap_or(Value::Null))
    }

    fn get_block_transactions(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let (skip, limit) =
            Self::parse_page(params, 1, STANDARD_PAGE_BOUNDS, "getblocktransactions")?;
        let Some(block_hash) =
            Self::block_hash_from_selector(&service, params, "getblocktransactions")?
        else {
            return Ok(Value::Array(Vec::new()));
        };

        Ok(Value::Array(
            service
                .try_transactions_for_block(&block_hash, skip, limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::transaction_to_json(&record, address_version))
                .collect(),
        ))
    }

    fn get_address_transactions(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let account = Self::expect_account(params, 0, "getaddresstransactions", address_version)?;
        let (skip, limit) =
            Self::parse_page(params, 1, STANDARD_PAGE_BOUNDS, "getaddresstransactions")?;

        let records = service
            .try_transactions_for_account(&account, skip, limit)
            .map_err(Self::indexer_error)?
            .into_iter()
            .map(|record| Self::account_transaction_to_json(&record, address_version))
            .collect::<Vec<_>>();
        Ok(Value::Array(records))
    }

    fn get_contract_transactions(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let (contract_hash, event_name, skip, limit) = Self::parse_contract_activity_params(
            params,
            address_version,
            "getcontracttransactions",
            STANDARD_PAGE_BOUNDS,
        )?;

        Ok(Value::Array(
            service
                .try_transactions_for_contract(&contract_hash, event_name.as_deref(), skip, limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::transaction_to_json(&record, address_version))
                .collect(),
        ))
    }

    fn get_address_notifications(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let account = Self::expect_account(params, 0, "getaddressnotifications", address_version)?;
        let (skip, limit) =
            Self::parse_page(params, 1, STANDARD_PAGE_BOUNDS, "getaddressnotifications")?;

        Ok(Value::Array(
            service
                .try_notifications_for_account(&account, skip, limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        ))
    }

    fn get_block_notifications(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let (skip, limit) =
            Self::parse_page(params, 1, STANDARD_PAGE_BOUNDS, "getblocknotifications")?;
        let Some(block_hash) =
            Self::block_hash_from_selector(&service, params, "getblocknotifications")?
        else {
            return Ok(Value::Array(Vec::new()));
        };

        Ok(Value::Array(
            service
                .try_notifications_for_block(&block_hash, skip, limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        ))
    }

    fn get_transaction_notifications(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let tx_hash = Self::expect_uint256(params, 0, "gettransactionnotifications")?;
        let (skip, limit) = Self::parse_page(
            params,
            1,
            STANDARD_PAGE_BOUNDS,
            "gettransactionnotifications",
        )?;

        Ok(Value::Array(
            service
                .try_notifications_for_transaction(&tx_hash, skip, limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        ))
    }

    fn get_contract_notifications(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let (contract_hash, event_name, skip, limit) = Self::parse_contract_activity_params(
            params,
            address_version,
            "getcontractnotifications",
            STANDARD_PAGE_BOUNDS,
        )?;

        Ok(Value::Array(
            service
                .try_notifications_for_contract(&contract_hash, event_name.as_deref(), skip, limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        ))
    }

    fn service(server: &RpcServer) -> Result<Arc<IndexerService>, RpcException> {
        server
            .system()
            .get_service::<IndexerService>()
            .ok_or_else(|| internal_error("NeoIndexer service not available"))
    }

    fn indexer_error(error: IndexerError) -> RpcException {
        internal_error(format!("NeoIndexer service read failed: {error}"))
    }
}

enum BlockSelector {
    Height(u32),
    Hash(UInt256),
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_indexer.rs"]
mod tests;
