use std::sync::Arc;

use neo_native_contracts::{NeoToken, ledger_contract::LedgerContract};
use neo_wallets::wallet_helper::WalletAddress as address_helper;
use serde_json::Value;

use crate::server::native_queries;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

use super::helpers::internal_error;
use super::request::GetUnclaimedGasRequest;
use super::response::unclaimed_gas_to_json;

pub(super) fn get_unclaimed_gas(
    server: &RpcServer,
    params: &[Value],
) -> Result<Value, RpcException> {
    let version = server.system().settings().address_version;
    let request = GetUnclaimedGasRequest::parse(params, version)?;

    let store = server.system().store_cache();
    let ledger = LedgerContract::new();
    let height = ledger
        .current_index(store.data_cache())
        .map_err(|err| internal_error(err.to_string()))?
        .saturating_add(1);
    let neo_hash = NeoToken::script_hash();
    let snapshot = Arc::new(store.data_cache().clone());
    let unclaimed = native_queries::NativeQueries::neo_unclaimed_gas(
        server,
        snapshot,
        &neo_hash,
        &request.script_hash,
        height,
    )
    .map_err(internal_error)?;
    let address = address_helper::to_address(&request.script_hash, version);

    Ok(unclaimed_gas_to_json(address, unclaimed))
}
