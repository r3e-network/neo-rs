//! Response construction helpers for wallet RPC methods.

use neo_primitives::BigDecimal;
use num_bigint::BigInt;
use serde_json::{Value, json};

pub(super) fn wallet_balance_to_json(balance: &BigDecimal) -> Value {
    json!({"balance": balance.to_string()})
}

pub(super) fn wallet_unclaimed_gas_to_json(total: &BigInt) -> Value {
    Value::String(total.to_string())
}

pub(super) fn network_fee_to_json(fee: i64) -> Value {
    json!({"networkfee": fee.to_string()})
}
