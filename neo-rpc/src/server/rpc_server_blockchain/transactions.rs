//! Transaction RPC handlers.
//!
//! This module owns transaction lookup and `getrawtransaction` response
//! projection. Request decoding stays in `request_helpers`, and the parent
//! blockchain module stays focused on handler registration plus remaining
//! legacy groups.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, serialize_to_base64};
use crate::server::rpc_server::RpcServer;
use neo_native_contracts::LedgerContract;
use serde_json::Value;

use super::RpcServerBlockchain;
use super::request_helpers::{RawTransactionRequest, TransactionHeightRequest};
use super::responses::{
    base64_payload_to_json, transaction_height_to_json, transaction_to_verbose_json,
};

impl RpcServerBlockchain {
    pub(super) fn get_raw_transaction(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getrawtransaction", params)
                .map_err(RpcException::from);
        }
        let request = RawTransactionRequest::parse(params)?;
        let system = server.system();

        let tx_from_pool = system.mempool().get(&request.hash);

        if !request.verbose {
            if let Some(item) = tx_from_pool {
                return Ok(base64_payload_to_json(serialize_to_base64(
                    item.transaction.as_ref(),
                )?));
            }
        }

        let store = system.store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(store.data_cache(), &request.hash)
            .map_err(internal_error)?;

        // Convert Arc<Transaction> to Transaction for uniform handling.
        let transaction = tx_from_pool
            .map(|item| (*item.transaction).clone())
            .or_else(|| state.as_ref().and_then(|state| state.transaction.clone()));
        let tx = transaction.ok_or_else(|| RpcException::from(RpcError::unknown_transaction()))?;

        if !request.verbose {
            return Ok(base64_payload_to_json(serialize_to_base64(&tx)?));
        }

        transaction_to_verbose_json(server, &tx, state.as_ref())
    }

    pub(super) fn get_transaction_height(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("gettransactionheight", params)
                .map_err(RpcException::from);
        }
        let request = TransactionHeightRequest::parse(params)?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(store.data_cache(), &request.hash)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_transaction()))?;
        Ok(transaction_height_to_json(state.block_index()))
    }
}
