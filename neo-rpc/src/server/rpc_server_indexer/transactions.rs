//! Transaction-index RPC endpoint handlers.

use serde_json::Value;

use super::params::{
    AccountPageRequest, BlockPageRequest, ContractActivityRequest, TransactionIndexRequest,
};
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
        let address_version = server.system().settings().address_version;
        let request = AccountPageRequest::parse(
            params,
            address_version,
            STANDARD_PAGE_BOUNDS,
            "getaddresstransactions",
        )?;
        let service = Self::service(server)?;

        let records = service
            .try_transactions_for_account(&request.account, request.page.skip, request.page.limit)
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
        let address_version = server.system().settings().address_version;
        let request = ContractActivityRequest::parse(
            params,
            address_version,
            STANDARD_PAGE_BOUNDS,
            "getcontracttransactions",
        )?;
        let service = Self::service(server)?;

        Ok(Value::Array(
            service
                .try_transactions_for_contract(
                    &request.contract_hash,
                    request.event_name.as_deref(),
                    request.page.skip,
                    request.page.limit,
                )
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::transaction_to_json(&record, address_version))
                .collect(),
        ))
    }
}
