use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, invalid_params};
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_script_builder::ScriptBuilder;
use neo_execution::application_engine::TEST_MODE_GAS;
use neo_manifest::CallFlags;
use neo_native_contracts::contract_management::ContractManagement;
use neo_execution::{ApplicationEngine, TriggerType};
use neo_tokens_tracker::{
    find_prefix, Nep11BalanceKey, Nep11Tracker, Nep17BalanceKey, Nep17Tracker, TokenBalance};
use neo_vm::stack_item::StackItem;
use neo_wallets::Helper as WalletHelper;
use neo_primitives::UInt160;
use neo_vm_rs::VmState as VMState;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

mod helpers;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use helpers::*;

pub struct RpcServerTokensTracker;

const NEP11_PROPERTIES: [&str; 4] = ["name", "description", "image", "tokenURI"];

impl RpcServerTokensTracker {
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
        let script_hash = parse_address_param(params, 0, "getnep11balances", address_version)?;

        let (balance_prefix, _, _) = Nep11Tracker::rpc_prefixes();
        let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
        prefix.push(balance_prefix);
        prefix.extend_from_slice(&script_hash.to_bytes());

        let balances =
            find_prefix::<Nep11BalanceKey, TokenBalance>(service.store().as_ref(), &prefix)
                .map_err(internal_error)?;

        let store_cache = server.system().store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let max_results = service.settings().max_results as usize;

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
                .push((hex::encode(&key.token), value));
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

            let Some((symbol, decimals)) =
                query_asset_metadata(snapshot.as_ref(), server.system().settings(), &asset)
            else {
                continue;
           };

            let token_entries = tokens
                .into_iter()
                .map(|(token_id, balance)| {
                    json!({
                        "tokenid": token_id,
                        "amount": balance.balance.to_string(),
                        "lastupdatedblock": balance.last_updated_block})
               })
                .collect::<Vec<_>>();

            results.push(json!({
                "assethash": asset.to_string(),
                "name": contract.manifest.name,
                "symbol": symbol,
                "decimals": decimals.to_string(),
                "tokens": token_entries}));
       }

        Ok(json!({
            "address": WalletHelper::to_address(&script_hash, address_version),
            "balance": results}))
   }

    fn get_nep11_transfers(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep11() || !service.settings().track_history {
            return Err(RpcException::from(RpcError::method_not_found()));
       }

        let address_version = server.system().settings().address_version;
        let script_hash = parse_address_param(params, 0, "getnep11transfers", address_version)?;

        let now_ms = current_time_millis();
        let start_time = parse_optional_u64(params.get(1))?;
        let end_time = parse_optional_u64(params.get(2))?;
        let start = if start_time == 0 {
            now_ms.saturating_sub(7 * 24 * 60 * 60 * 1000)
       } else {
            start_time
       };
        let end = if end_time == 0 {now_ms} else {end_time};
        if end < start {
            return Err(invalid_params("endTime must be >= startTime"));
       }

        let (_, sent_prefix, received_prefix) = Nep11Tracker::rpc_prefixes();
        let max_results = service.settings().max_results as usize;

        let sent = collect_nep11_transfers(
            service.store().as_ref(),
            sent_prefix,
            &script_hash,
            start,
            end,
            address_version,
            max_results,
        )?;
        let received = collect_nep11_transfers(
            service.store().as_ref(),
            received_prefix,
            &script_hash,
            start,
            end,
            address_version,
            max_results,
        )?;

        Ok(json!({
            "address": WalletHelper::to_address(&script_hash, address_version),
            "sent": sent,
            "received": received}))
   }

    fn get_nep11_properties(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep11() {
            return Err(RpcException::from(RpcError::method_not_found()));
       }

        let address_version = server.system().settings().address_version;
        let script_hash = parse_address_param(params, 0, "getnep11properties", address_version)?;
        let token_id = parse_token_id_param(params, 1, "getnep11properties")?;

        let mut script = ScriptBuilder::new();
        emit_contract_call_with_arg(
            &mut script,
            &script_hash,
            "properties",
            CallFlags::READ_ONLY,
            &token_id,
        )?;

        let store_cache = server.system().store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot,
            None,
            server.system().settings().clone(),
            TEST_MODE_GAS,
            None,
        )
        .map_err(|err| internal_error(err.to_string()))?;
        engine
            .load_script(script.to_array(), CallFlags::ALL, Some(script_hash))
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
        let script_hash = parse_address_param(params, 0, "getnep17balances", address_version)?;

        let (balance_prefix, _, _) = Nep17Tracker::rpc_prefixes();
        let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
        prefix.push(balance_prefix);
        prefix.extend_from_slice(&script_hash.to_bytes());

        let balances =
            find_prefix::<Nep17BalanceKey, TokenBalance>(service.store().as_ref(), &prefix)
                .map_err(internal_error)?;

        let store_cache = server.system().store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let mut results = Vec::new();
        let max_results = service.settings().max_results as usize;

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
                server.system().settings(),
                &key.asset_script_hash,
            ) else {
                continue;
           };

            results.push(json!({
                "assethash": key.asset_script_hash.to_string(),
                "name": contract.manifest.name,
                "symbol": symbol,
                "decimals": decimals.to_string(),
                "amount": value.balance.to_string(),
                "lastupdatedblock": value.last_updated_block}));
       }

        Ok(json!({
            "address": WalletHelper::to_address(&script_hash, address_version),
            "balance": results}))
   }

    fn get_nep17_transfers(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep17() || !service.settings().track_history {
            return Err(RpcException::from(RpcError::method_not_found()));
       }

        let address_version = server.system().settings().address_version;
        let script_hash = parse_address_param(params, 0, "getnep17transfers", address_version)?;

        let now_ms = current_time_millis();
        let start_time = parse_optional_u64(params.get(1))?;
        let end_time = parse_optional_u64(params.get(2))?;
        let start = if start_time == 0 {
            now_ms.saturating_sub(7 * 24 * 60 * 60 * 1000)
       } else {
            start_time
       };
        let end = if end_time == 0 {now_ms} else {end_time};
        if end < start {
            return Err(invalid_params("endTime must be >= startTime"));
       }

        let (_, sent_prefix, received_prefix) = Nep17Tracker::rpc_prefixes();
        let max_results = service.settings().max_results as usize;

        let sent = collect_transfers(
            service.store().as_ref(),
            sent_prefix,
            &script_hash,
            start,
            end,
            address_version,
            max_results,
        )?;
        let received = collect_transfers(
            service.store().as_ref(),
            received_prefix,
            &script_hash,
            start,
            end,
            address_version,
            max_results,
        )?;

        Ok(json!({
            "address": WalletHelper::to_address(&script_hash, address_version),
            "sent": sent,
            "received": received}))
   }
}
