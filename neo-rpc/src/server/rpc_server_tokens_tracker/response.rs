//! Typed response construction for token-tracker RPC handlers.

use crate::plugins::tokens_tracker::trackers::tracker_base::TokenTransferKeyView;
use crate::plugins::tokens_tracker::{Nep11TransferKey, TokenBalance, TokenTransfer};
use neo_primitives::{UInt160, hex_util};
use serde_json::{Value, json};

pub(super) fn account_balances(
    script_hash: &UInt160,
    address_version: u8,
    balances: Vec<Value>,
) -> Value {
    json!({
        "address": neo_wallets::wallet_helper::WalletAddress::to_address(script_hash, address_version),
        "balance": balances
    })
}

pub(super) fn transfer_history(
    script_hash: &UInt160,
    address_version: u8,
    sent: Value,
    received: Value,
) -> Value {
    json!({
        "address": neo_wallets::wallet_helper::WalletAddress::to_address(script_hash, address_version),
        "sent": sent,
        "received": received
    })
}

pub(super) fn transfer_entries(entries: Vec<Value>) -> Value {
    Value::Array(entries)
}

pub(super) fn transfer_entry<K>(key: &K, value: &TokenTransfer, address_version: u8) -> Value
where
    K: TokenTransferKeyView,
{
    let transfer_address = if value.user_script_hash == UInt160::zero() {
        Value::Null
    } else {
        Value::String(neo_wallets::wallet_helper::WalletAddress::to_address(
            &value.user_script_hash,
            address_version,
        ))
    };

    json!({
        "timestamp": key.timestamp_ms(),
        "assethash": key.asset_script_hash().to_string(),
        "transferaddress": transfer_address,
        "amount": value.amount.to_string(),
        "blockindex": value.block_index,
        "transfernotifyindex": key.block_xfer_notification_index(),
        "txhash": value.tx_hash.to_string(),
    })
}

pub(super) fn nep11_transfer_entry(
    key: &Nep11TransferKey,
    value: &TokenTransfer,
    address_version: u8,
) -> Value {
    let mut entry = transfer_entry(key, value, address_version);
    if let Value::Object(ref mut object) = entry {
        object.insert(
            "tokenid".to_string(),
            Value::String(hex_util::encode_hex(&key.token)),
        );
    }
    entry
}

pub(super) fn nep17_balance_entry(
    asset: &UInt160,
    name: &str,
    symbol: &str,
    decimals: u32,
    balance: &TokenBalance,
) -> Value {
    json!({
        "assethash": asset.to_string(),
        "name": name,
        "symbol": symbol,
        "decimals": decimals.to_string(),
        "amount": balance.balance.to_string(),
        "lastupdatedblock": balance.last_updated_block
    })
}

pub(super) fn nep11_balance_entry(
    asset: &UInt160,
    name: &str,
    symbol: &str,
    decimals: u32,
    tokens: Vec<Value>,
) -> Value {
    json!({
        "assethash": asset.to_string(),
        "name": name,
        "symbol": symbol,
        "decimals": decimals.to_string(),
        "tokens": tokens
    })
}

pub(super) fn nep11_token_entry(token_id: String, balance: TokenBalance) -> Value {
    json!({
        "tokenid": token_id,
        "amount": balance.balance.to_string(),
        "lastupdatedblock": balance.last_updated_block
    })
}
