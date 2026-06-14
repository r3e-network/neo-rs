use std::str::FromStr;
use std::sync::Arc;

use neo_native_contracts::{NeoToken, ledger_contract::LedgerContract};
use neo_primitives::UInt160;
use neo_wallets::wallet_helper::WalletAddress as address_helper;
use serde_json::{Value, json};

use crate::server::native_queries;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

use super::helpers::{expect_string_param, internal_error, invalid_params};

pub(super) fn get_unclaimed_gas(
    server: &RpcServer,
    params: &[Value],
) -> Result<Value, RpcException> {
    let address_text = expect_string_param(params, 0, "getunclaimedgas")?;
    let version = server.system().settings().address_version;
    let script_hash = if let Ok(hash) = UInt160::from_str(&address_text) {
        hash
    } else {
        address_helper::to_script_hash(&address_text, version)
            .map_err(|e| invalid_params(e.to_string()))?
    };

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
        &script_hash,
        height,
    )
    .map_err(internal_error)?;
    let address = address_helper::to_address(&script_hash, version);

    Ok(json!({
         "address": address,
         // C# GetUnclaimedGas returns the raw datoshi BigInteger as a string
         // (NEO.UnclaimedGas(...).ToString()), e.g. "100000000" for 1 GAS — not
         // the decimal form. Wrapping in BigDecimal would divide by 10^8.
         "unclaimed": unclaimed.to_string()
    }))
}
