//! Notification-index RPC endpoint handlers.

use serde_json::Value;

use super::{RpcServerIndexer, STANDARD_PAGE_BOUNDS};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

impl RpcServerIndexer {
    pub(super) fn get_address_notifications(
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

    pub(super) fn get_block_notifications(
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

    pub(super) fn get_transaction_notifications(
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

    pub(super) fn get_contract_notifications(
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
}
