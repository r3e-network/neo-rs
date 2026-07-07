//! Typed response construction for token-tracker RPC handlers.

use crate::plugins::tokens_tracker::TokenBalance;
use neo_primitives::UInt160;
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
