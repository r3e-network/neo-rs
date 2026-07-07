//! Response construction helpers for wallet RPC methods.

use neo_primitives::BigDecimal;
use neo_wallets::WalletAccount;
use num_bigint::BigInt;
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub(super) fn wallet_success_to_json() -> Value {
    Value::Bool(true)
}

pub(super) fn wallet_secret_to_json(wif: String) -> Value {
    Value::String(wif)
}

pub(super) fn wallet_address_to_json(address: String) -> Value {
    Value::String(address)
}

pub(super) fn wallet_account_to_json(account: &(impl WalletAccount + ?Sized)) -> Value {
    let has_key = account.has_key();
    let mut map = Map::new();
    map.insert("address".to_string(), Value::String(account.address()));
    map.insert("haskey".to_string(), Value::Bool(has_key));
    map.insert(
        "label".to_string(),
        account
            .label()
            .map_or(Value::Null, |label| Value::String(label.to_string())),
    );
    map.insert("watchonly".to_string(), Value::Bool(!has_key));
    Value::Object(map)
}

pub(super) fn wallet_accounts_to_json(accounts: Vec<Arc<dyn WalletAccount>>) -> Value {
    Value::Array(
        accounts
            .into_iter()
            .map(|account| wallet_account_to_json(account.as_ref()))
            .collect(),
    )
}

pub(super) fn wallet_balance_to_json(balance: &BigDecimal) -> Value {
    json!({"balance": balance.to_string()})
}

pub(super) fn wallet_unclaimed_gas_to_json(total: &BigInt) -> Value {
    Value::String(total.to_string())
}

pub(super) fn network_fee_to_json(fee: i64) -> Value {
    json!({"networkfee": fee.to_string()})
}
