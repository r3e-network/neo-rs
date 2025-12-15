use std::str::FromStr;

use neo_core::big_decimal::BigDecimal;
use neo_core::smart_contract::native::{ledger_contract::LedgerContract, NeoToken};
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::UInt160;
use serde_json::{json, Value};

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
        WalletHelper::to_script_hash(&address_text, version).map_err(invalid_params)?
    };

    let store = server.system().store_cache();
    let ledger = LedgerContract::new();
    let height = ledger
        .current_index(&store)
        .map_err(|err| internal_error(err.to_string()))?
        .saturating_add(1);
    let neo = NeoToken::new();
    let unclaimed = neo
        .unclaimed_gas(&store, &script_hash, height)
        .map_err(|err| internal_error(err.to_string()))?;

    Ok(json!({
        "address": script_hash.to_string(),
        "unclaimed": BigDecimal::new(unclaimed, neo.decimals()).to_string()
    }))
}
