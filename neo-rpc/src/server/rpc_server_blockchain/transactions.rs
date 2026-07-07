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
use serde_json::{Value, json};

use super::RpcServerBlockchain;
use super::request_helpers::RawTransactionRequest;

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
                return Ok(Value::String(serialize_to_base64(
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
            return Ok(Value::String(serialize_to_base64(&tx)?));
        }

        let settings = system.settings();
        let mut json = tx.to_json(&settings);
        if let (Value::Object(obj), Some(state)) = (&mut json, state) {
            let block_index = state.block_index();
            let current_index = ledger
                .current_index(store.data_cache())
                .map_err(internal_error)?;
            let confirmations = current_index.saturating_sub(block_index).saturating_add(1);
            obj.insert("confirmations".to_string(), json!(confirmations));

            // C# GetRawTransaction verbose adds only blockhash, confirmations and
            // blocktime to Transaction.ToJson (RpcServer.Blockchain.cs:373-381);
            // it does NOT add a vmstate field (that belongs to getapplicationlog).
            // Emitting it here surprises strict clients / response-diff tooling.
            if let Some(block_hash) = ledger
                .get_block_hash(store.data_cache(), block_index)
                .map_err(internal_error)?
            {
                obj.insert(
                    "blockhash".to_string(),
                    Value::String(block_hash.to_string()),
                );

                if let Some(block) = ledger
                    .get_trimmed_block(store.data_cache(), &block_hash)
                    .map_err(internal_error)?
                {
                    obj.insert("blocktime".to_string(), json!(block.header.timestamp()));
                }
            }
        }

        Ok(json)
    }
}
