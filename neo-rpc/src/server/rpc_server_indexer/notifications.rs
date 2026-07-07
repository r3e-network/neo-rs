//! Notification-index RPC endpoint handlers.

use serde_json::Value;

use super::params::{
    AccountPageRequest, BlockPageRequest, ContractActivityRequest, TransactionPageRequest,
};
use super::{RpcServerIndexer, STANDARD_PAGE_BOUNDS};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

impl RpcServerIndexer {
    pub(super) fn get_address_notifications(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let address_version = server.system().settings().address_version;
        let request = AccountPageRequest::parse(
            params,
            address_version,
            STANDARD_PAGE_BOUNDS,
            "getaddressnotifications",
        )?;
        let service = Self::service(server)?;

        Ok(Value::Array(
            service
                .try_notifications_for_account(
                    &request.account,
                    request.page.skip,
                    request.page.limit,
                )
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        ))
    }

    pub(super) fn get_block_notifications(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request =
            BlockPageRequest::parse(params, STANDARD_PAGE_BOUNDS, "getblocknotifications")?;
        let service = Self::service(server)?;
        let address_version = server.system().settings().address_version;
        let Some(block_hash) = Self::block_hash_from_selector_value(&service, request.selector)?
        else {
            return Ok(Value::Array(Vec::new()));
        };

        Ok(Value::Array(
            service
                .try_notifications_for_block(&block_hash, request.page.skip, request.page.limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        ))
    }

    pub(super) fn get_transaction_notifications(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = TransactionPageRequest::parse(
            params,
            STANDARD_PAGE_BOUNDS,
            "gettransactionnotifications",
        )?;
        let address_version = server.system().settings().address_version;
        let service = Self::service(server)?;

        Ok(Value::Array(
            service
                .try_notifications_for_transaction(
                    &request.hash,
                    request.page.skip,
                    request.page.limit,
                )
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        ))
    }

    pub(super) fn get_contract_notifications(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let address_version = server.system().settings().address_version;
        let request = ContractActivityRequest::parse(
            params,
            address_version,
            STANDARD_PAGE_BOUNDS,
            "getcontractnotifications",
        )?;
        let service = Self::service(server)?;

        Ok(Value::Array(
            service
                .try_notifications_for_contract(
                    &request.contract_hash,
                    request.event_name.as_deref(),
                    request.page.skip,
                    request.page.limit,
                )
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        ))
    }
}
