//! # neo-rpc::server::rpc_server_tokens_tracker
//!
//! Token tracker RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `helpers`: Shared helper functions for the surrounding module.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Typed JSON-RPC response construction helpers.
//! - `tests`: Module-local tests and regression coverage.

use crate::plugins::tokens_tracker::{
    Nep11BalanceKey, Nep11Tracker, Nep17BalanceKey, Nep17Tracker, TokenBalance, find_prefix,
};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_execution::application_engine::TEST_MODE_GAS;
use neo_execution::{ApplicationEngine, TriggerType};
use neo_manifest::CallFlags;
use neo_native_contracts::contract_management::ContractManagement;
use neo_primitives::UInt160;
use neo_primitives::hex_util;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::stack_item::StackItem;
use neo_vm_rs::VmState as VMState;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

mod helpers;
mod request;
mod response;
#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_tokens_tracker.rs"]
mod tests;

use helpers::*;
use request::{AccountRequest, Nep11PropertiesRequest, TransferHistoryRequest};
use response::{
    account_balances, nep11_balance_entry, nep11_token_entry, nep17_balance_entry, transfer_history,
};

/// RPC handler group for NEP-11 and NEP-17 token tracker methods.
pub struct RpcServerTokensTracker;

const NEP11_PROPERTIES: [&str; 4] = ["name", "description", "image", "tokenURI"];

impl RpcServerTokensTracker {
    /// Register token tracker RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getnep11balances" => Self::get_nep11_balances,
            "getnep11transfers" => Self::get_nep11_transfers,
            "getnep11properties" => Self::get_nep11_properties,
            "getnep17balances" => Self::get_nep17_balances,
            "getnep17transfers" => Self::get_nep17_transfers,
        ]
    }

    fn get_nep11_balances(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep11() {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = AccountRequest::parse(params, "getnep11balances", address_version)?;

        let (balance_prefix, _, _) = Nep11Tracker::rpc_prefixes();
        let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
        prefix.push(balance_prefix);
        prefix.extend_from_slice(&request.script_hash.to_bytes());

        let balances =
            find_prefix::<_, Nep11BalanceKey, TokenBalance>(service.store().as_ref(), &prefix)
                .map_err(internal_error)?;

        let store_cache = server.system().store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let max_results = service.settings().max_results_limit();

        let mut grouped: HashMap<UInt160, Vec<(String, TokenBalance)>> = HashMap::new();
        let mut count = 0usize;

        for (key, value) in balances {
            if count >= max_results {
                break;
            }
            let Some(_) = ContractManagement::get_contract_from_snapshot(
                snapshot.as_ref(),
                &key.asset_script_hash,
            )
            .map_err(|err| internal_error(err.to_string()))?
            else {
                continue;
            };

            grouped
                .entry(key.asset_script_hash)
                .or_default()
                .push((hex_util::encode_hex(&key.token), value));
            count += 1;
        }

        let mut results = Vec::new();
        for (asset, tokens) in grouped {
            let Some(contract) =
                ContractManagement::get_contract_from_snapshot(snapshot.as_ref(), &asset)
                    .map_err(|err| internal_error(err.to_string()))?
            else {
                continue;
            };

            let Some((symbol, decimals)) = query_asset_metadata(
                snapshot.as_ref(),
                &server.system().settings(),
                server.system().native_contract_provider(),
                &asset,
            ) else {
                continue;
            };

            let token_entries = tokens
                .into_iter()
                .map(|(token_id, balance)| nep11_token_entry(token_id, balance))
                .collect::<Vec<_>>();

            results.push(nep11_balance_entry(
                &asset,
                &contract.manifest.name,
                &symbol,
                decimals,
                token_entries,
            ));
        }

        Ok(account_balances(
            &request.script_hash,
            address_version,
            results,
        ))
    }

    fn get_nep11_transfers(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep11() || !service.settings().track_history {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = TransferHistoryRequest::parse(params, "getnep11transfers", address_version)?;

        let (_, sent_prefix, received_prefix) = Nep11Tracker::rpc_prefixes();
        let max_results = service.settings().max_results_limit();

        let sent = collect_nep11_transfers(
            service.store().as_ref(),
            sent_prefix,
            &request.script_hash,
            request.start,
            request.end,
            address_version,
            max_results,
        )?;
        let received = collect_nep11_transfers(
            service.store().as_ref(),
            received_prefix,
            &request.script_hash,
            request.start,
            request.end,
            address_version,
            max_results,
        )?;

        Ok(transfer_history(
            &request.script_hash,
            address_version,
            sent,
            received,
        ))
    }

    fn get_nep11_properties(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep11() {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = Nep11PropertiesRequest::parse(params, address_version)?;

        let mut script = ScriptBuilder::new();
        emit_contract_call_with_arg(
            &mut script,
            &request.script_hash,
            "properties",
            CallFlags::READ_ONLY,
            &request.token_id,
        )?;

        let system = server.system();
        let store_cache = system.store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            None,
            snapshot,
            None,
            system.settings().as_ref().clone(),
            TEST_MODE_GAS,
            None,
            Some(system.native_contract_provider()),
        )
        .map_err(|err| internal_error(err.to_string()))?;
        engine
            .load_script(script.to_array(), CallFlags::ALL, Some(request.script_hash))
            .map_err(|err| internal_error(err.to_string()))?;
        engine
            .execute()
            .map_err(|err| internal_error(err.to_string()))?;

        if engine.state() != VMState::HALT {
            return Ok(Value::Object(Map::new()));
        }

        let map_item = engine
            .result_stack()
            .peek(0)
            .map_err(|err| internal_error(err.to_string()))?
            .clone();
        let map = map_item
            .as_map()
            .map_err(|err| internal_error(err.to_string()))?;

        let mut result = Map::new();
        for (key, value) in map.iter() {
            if matches!(
                value,
                StackItem::Array(_) | StackItem::Struct(_) | StackItem::Map(_)
            ) {
                continue;
            }

            let key_bytes = key
                .as_bytes()
                .map_err(|_| internal_error("unexpected null key"))?;
            let key_text =
                String::from_utf8(key_bytes).map_err(|err| internal_error(err.to_string()))?;

            if NEP11_PROPERTIES.iter().any(|prop| *prop == key_text) {
                if matches!(value, StackItem::Null) {
                    result.insert(key_text, Value::Null);
                } else {
                    let value_bytes = value
                        .as_bytes()
                        .map_err(|err| internal_error(err.to_string()))?;
                    let text = String::from_utf8(value_bytes)
                        .map_err(|err| internal_error(err.to_string()))?;
                    result.insert(key_text, Value::String(text));
                }
            } else if matches!(value, StackItem::Null) {
                result.insert(key_text, Value::Null);
            } else {
                let value_bytes = value
                    .as_bytes()
                    .map_err(|err| internal_error(err.to_string()))?;
                let encoded = BASE64_STANDARD.encode(value_bytes);
                result.insert(key_text, Value::String(encoded));
            }
        }

        Ok(Value::Object(result))
    }

    fn get_nep17_balances(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep17() {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = AccountRequest::parse(params, "getnep17balances", address_version)?;

        let (balance_prefix, _, _) = Nep17Tracker::rpc_prefixes();
        let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
        prefix.push(balance_prefix);
        prefix.extend_from_slice(&request.script_hash.to_bytes());

        let balances =
            find_prefix::<_, Nep17BalanceKey, TokenBalance>(service.store().as_ref(), &prefix)
                .map_err(internal_error)?;

        let store_cache = server.system().store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let mut results = Vec::new();
        let max_results = service.settings().max_results_limit();

        for (key, value) in balances {
            if results.len() >= max_results {
                break;
            }
            let Some(contract) = ContractManagement::get_contract_from_snapshot(
                snapshot.as_ref(),
                &key.asset_script_hash,
            )
            .map_err(|err| internal_error(err.to_string()))?
            else {
                continue;
            };

            let Some((symbol, decimals)) = query_asset_metadata(
                snapshot.as_ref(),
                &server.system().settings(),
                server.system().native_contract_provider(),
                &key.asset_script_hash,
            ) else {
                continue;
            };

            results.push(nep17_balance_entry(
                &key.asset_script_hash,
                &contract.manifest.name,
                &symbol,
                decimals,
                &value,
            ));
        }

        Ok(account_balances(
            &request.script_hash,
            address_version,
            results,
        ))
    }

    fn get_nep17_transfers(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep17() || !service.settings().track_history {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = TransferHistoryRequest::parse(params, "getnep17transfers", address_version)?;

        let (_, sent_prefix, received_prefix) = Nep17Tracker::rpc_prefixes();
        let max_results = service.settings().max_results_limit();

        let sent = collect_transfers(
            service.store().as_ref(),
            sent_prefix,
            &request.script_hash,
            request.start,
            request.end,
            address_version,
            max_results,
        )?;
        let received = collect_transfers(
            service.store().as_ref(),
            received_prefix,
            &request.script_hash,
            request.start,
            request.end,
            address_version,
            max_results,
        )?;

        Ok(transfer_history(
            &request.script_hash,
            address_version,
            sent,
            received,
        ))
    }
}
