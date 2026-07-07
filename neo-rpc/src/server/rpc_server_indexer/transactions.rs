//! Transaction-index RPC endpoint handlers.

use serde_json::Value;

use super::params::{BlockPageRequest, TransactionIndexRequest};
use super::{RpcServerIndexer, STANDARD_PAGE_BOUNDS};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

impl RpcServerIndexer {
    pub(super) fn get_transaction_index(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = TransactionIndexRequest::parse(params)?;
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        Ok(service
            .try_transaction(&request.hash)
            .map_err(Self::indexer_error)?
            .map(|record| Self::transaction_to_json(&record, address_version))
            .unwrap_or(Value::Null))
    }

    pub(super) fn get_block_transactions(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request =
            BlockPageRequest::parse(params, STANDARD_PAGE_BOUNDS, "getblocktransactions")?;
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let Some(block_hash) = Self::block_hash_from_selector_value(&service, request.selector)?
        else {
            return Ok(Value::Array(Vec::new()));
        };

        Ok(Value::Array(
            service
                .try_transactions_for_block(&block_hash, request.page.skip, request.page.limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::transaction_to_json(&record, address_version))
                .collect(),
        ))
    }

    pub(super) fn get_address_transactions(
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

    pub(super) fn get_contract_transactions(
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
}
