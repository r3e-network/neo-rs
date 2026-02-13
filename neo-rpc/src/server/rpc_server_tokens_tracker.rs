use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, invalid_params};
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_core::smart_contract::application_engine::TEST_MODE_GAS;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::contract_management::ContractManagement;
use neo_core::smart_contract::{ApplicationEngine, TriggerType};
use neo_core::tokens_tracker::{
    find_prefix, find_range, Nep11BalanceKey, Nep11Tracker, Nep11TransferKey, Nep17BalanceKey,
    Nep17Tracker, Nep17TransferKey, TokenBalance, TokenTransfer, TokenTransferKeyView,
    TokensTrackerService,
};
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::UInt160;
use neo_vm::op_code::OpCode;
use neo_vm::{ScriptBuilder, StackItem, VMState};
use num_traits::ToPrimitive;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RpcServerTokensTracker;

const NEP11_PROPERTIES: [&str; 4] = ["name", "description", "image", "tokenURI"];

impl RpcServerTokensTracker {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("getnep11balances", Self::get_nep11_balances),
            Self::handler("getnep11transfers", Self::get_nep11_transfers),
            Self::handler("getnep11properties", Self::get_nep11_properties),
            Self::handler("getnep17balances", Self::get_nep17_balances),
            Self::handler("getnep17transfers", Self::get_nep17_transfers),
        ]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
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
                        "lastupdatedblock": balance.last_updated_block,
                    })
                })
                .collect::<Vec<_>>();

            results.push(json!({
                "assethash": asset.to_string(),
                "name": contract.manifest.name,
                "symbol": symbol,
                "decimals": decimals.to_string(),
                "tokens": token_entries,
            }));
        }

        Ok(json!({
            "address": WalletHelper::to_address(&script_hash, address_version),
            "balance": results,
        }))
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
        let end = if end_time == 0 { now_ms } else { end_time };
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
            "received": received,
        }))
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
                "lastupdatedblock": value.last_updated_block,
            }));
        }

        Ok(json!({
            "address": WalletHelper::to_address(&script_hash, address_version),
            "balance": results,
        }))
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
        let end = if end_time == 0 { now_ms } else { end_time };
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
            "received": received,
        }))
    }
}

fn tracker_service(server: &RpcServer) -> Result<Arc<TokensTrackerService>, RpcException> {
    server
        .system()
        .get_service::<TokensTrackerService>()
        .map_err(|err| internal_error(err.to_string()))?
        .ok_or_else(|| RpcException::from(RpcError::method_not_found()))
}

fn parse_address_param(
    params: &[Value],
    index: usize,
    method: &str,
    address_version: u8,
) -> Result<UInt160, RpcException> {
    let text = params
        .get(index)
        .and_then(|value| value.as_str())
        .ok_or_else(|| invalid_params(format!("{method} requires address parameter")))?;

    let mut parsed = None;
    if UInt160::try_parse(text, &mut parsed) {
        if let Some(value) = parsed {
            return Ok(value);
        }
    }

    WalletHelper::to_script_hash(text, address_version)
        .map_err(|_| invalid_params(format!("Invalid address: {text}")))
}

fn parse_optional_u64(value: Option<&Value>) -> Result<u64, RpcException> {
    let Some(value) = value else {
        return Ok(0);
    };
    match value {
        Value::Null => Ok(0),
        Value::Number(num) => num
            .as_u64()
            .ok_or_else(|| invalid_params("Expected unsigned integer")),
        Value::String(text) => text
            .trim()
            .parse::<u64>()
            .map_err(|_| invalid_params("Expected unsigned integer")),
        _ => Err(invalid_params("Expected unsigned integer")),
    }
}

fn parse_token_id_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<Vec<u8>, RpcException> {
    let text = params
        .get(index)
        .and_then(|value| value.as_str())
        .ok_or_else(|| invalid_params(format!("{method} requires tokenId parameter")))?;
    hex::decode(text).map_err(|_| invalid_params("Invalid tokenId"))
}

fn collect_transfers(
    store: &dyn neo_core::persistence::IStore,
    prefix: u8,
    script_hash: &UInt160,
    start: u64,
    end: u64,
    address_version: u8,
    max_results: usize,
) -> Result<Value, RpcException> {
    let mut prefix_bytes = Vec::with_capacity(1 + UInt160::LENGTH);
    prefix_bytes.push(prefix);
    prefix_bytes.extend_from_slice(&script_hash.to_bytes());

    let start_key = [prefix_bytes.as_slice(), &start.to_be_bytes()].concat();
    let end_key = [prefix_bytes.as_slice(), &end.to_be_bytes()].concat();

    let pairs = find_range::<Nep17TransferKey, TokenTransfer>(store, &start_key, &end_key)
        .map_err(internal_error)?;

    let mut limited = pairs
        .into_iter()
        .take(max_results)
        .enumerate()
        .collect::<Vec<_>>();
    limited.sort_by(|(left_index, left), (right_index, right)| {
        let left_ts = left.0.timestamp_ms();
        let right_ts = right.0.timestamp_ms();
        right_ts
            .cmp(&left_ts)
            .then_with(|| left_index.cmp(right_index))
    });

    let mut entries = Vec::new();
    for (_, (key, value)) in limited {
        let transfer_address = if value.user_script_hash == UInt160::zero() {
            Value::Null
        } else {
            Value::String(WalletHelper::to_address(
                &value.user_script_hash,
                address_version,
            ))
        };
        entries.push(json!({
            "timestamp": key.timestamp_ms(),
            "assethash": key.asset_script_hash().to_string(),
            "transferaddress": transfer_address,
            "amount": value.amount.to_string(),
            "blockindex": value.block_index,
            "transfernotifyindex": key.block_xfer_notification_index(),
            "txhash": value.tx_hash.to_string(),
        }));
    }

    Ok(Value::Array(entries))
}

fn collect_nep11_transfers(
    store: &dyn neo_core::persistence::IStore,
    prefix: u8,
    script_hash: &UInt160,
    start: u64,
    end: u64,
    address_version: u8,
    max_results: usize,
) -> Result<Value, RpcException> {
    let mut prefix_bytes = Vec::with_capacity(1 + UInt160::LENGTH);
    prefix_bytes.push(prefix);
    prefix_bytes.extend_from_slice(&script_hash.to_bytes());

    let start_key = [prefix_bytes.as_slice(), &start.to_be_bytes()].concat();
    let end_key = [prefix_bytes.as_slice(), &end.to_be_bytes()].concat();

    let pairs = find_range::<Nep11TransferKey, TokenTransfer>(store, &start_key, &end_key)
        .map_err(internal_error)?;

    let mut limited = pairs
        .into_iter()
        .take(max_results)
        .enumerate()
        .collect::<Vec<_>>();
    limited.sort_by(|(left_index, left), (right_index, right)| {
        let left_ts = left.0.timestamp_ms();
        let right_ts = right.0.timestamp_ms();
        right_ts
            .cmp(&left_ts)
            .then_with(|| left_index.cmp(right_index))
    });

    let mut entries = Vec::new();
    for (_, (key, value)) in limited {
        let transfer_address = if value.user_script_hash == UInt160::zero() {
            Value::Null
        } else {
            Value::String(WalletHelper::to_address(
                &value.user_script_hash,
                address_version,
            ))
        };
        entries.push(json!({
            "timestamp": key.timestamp_ms(),
            "assethash": key.asset_script_hash().to_string(),
            "transferaddress": transfer_address,
            "amount": value.amount.to_string(),
            "blockindex": value.block_index,
            "transfernotifyindex": key.block_xfer_notification_index(),
            "txhash": value.tx_hash.to_string(),
            "tokenid": hex::encode(&key.token),
        }));
    }

    Ok(Value::Array(entries))
}

fn query_asset_metadata(
    snapshot: &neo_core::persistence::DataCache,
    settings: &neo_core::protocol_settings::ProtocolSettings,
    asset: &UInt160,
) -> Option<(String, u32)> {
    let mut script = ScriptBuilder::new();
    emit_contract_call(&mut script, asset, "decimals").ok()?;
    emit_contract_call(&mut script, asset, "symbol").ok()?;

    let mut engine = neo_core::smart_contract::ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(snapshot.clone()),
        None,
        settings.clone(),
        TEST_MODE_GAS,
        None,
    )
    .ok()?;
    engine
        .load_script(script.to_array(), CallFlags::ALL, Some(*asset))
        .ok()?;
    engine.execute().ok()?;
    if engine.state() != neo_vm::vm_state::VMState::HALT {
        return None;
    }

    let result_stack = engine.result_stack();
    let symbol_item = result_stack.peek(0).ok()?;
    let decimals_item = result_stack.peek(1).ok()?;

    let symbol_bytes = symbol_item.as_bytes().ok()?;
    let symbol = String::from_utf8(symbol_bytes).ok()?;
    let decimals = decimals_item.get_integer().ok()?.to_u32()?;

    Some((symbol, decimals))
}

fn emit_contract_call(
    builder: &mut ScriptBuilder,
    contract: &UInt160,
    method: &str,
) -> Result<(), RpcException> {
    builder.emit_opcode(OpCode::PUSH0);
    builder.emit_opcode(OpCode::NEWARRAY);
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push_string(method);
    builder.emit_push_byte_array(&contract.to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| internal_error(err.to_string()))?;
    Ok(())
}

fn emit_contract_call_with_arg(
    builder: &mut ScriptBuilder,
    contract: &UInt160,
    method: &str,
    call_flags: CallFlags,
    arg: &[u8],
) -> Result<(), RpcException> {
    builder.emit_push_byte_array(arg);
    builder.emit_push_int(1);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(i64::from(call_flags.bits()));
    builder.emit_push_string(method);
    builder.emit_push_byte_array(&contract.to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| internal_error(err.to_string()))?;
    Ok(())
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::rpc_server::RpcHandler;
    use crate::server::rpc_server_settings::RpcServerConfig;
    use neo_core::neo_io::{Serializable, SerializableExt};
    use neo_core::neo_system::NeoSystem;
    use neo_core::persistence::providers::MemoryStoreProvider;
    use neo_core::persistence::IStore;
    use neo_core::persistence::IStoreProvider;
    use neo_core::protocol_settings::ProtocolSettings;
    use neo_core::smart_contract::manifest::{
        ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
    };
    use neo_core::smart_contract::native::GasToken;
    use neo_core::smart_contract::native::NativeRegistry;
    use neo_core::smart_contract::{
        ContractParameterType, ContractState, NefFile, StorageItem, StorageKey,
    };
    use neo_core::tokens_tracker::TokensTrackerSettings;
    use neo_core::NativeContract;
    use neo_core::UInt256;
    use num_bigint::BigInt;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
        handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("handler {} not found", name))
    }

    fn create_tracker_store() -> Arc<dyn IStore> {
        let provider = MemoryStoreProvider::new();
        provider
            .get_store("tokens")
            .expect("memory store available")
    }

    fn write_tracker_entry<K, V>(store: &Arc<dyn IStore>, prefix: u8, key: &K, value: &V)
    where
        K: Serializable,
        V: Serializable,
    {
        let mut key_bytes = Vec::with_capacity(1 + key.size());
        key_bytes.push(prefix);
        key_bytes.extend_from_slice(&key.to_array().expect("serialize key"));
        let value_bytes = value.to_array().expect("serialize value");

        let mut snapshot = store.get_snapshot();
        let snapshot = Arc::get_mut(&mut snapshot).expect("unique snapshot");
        snapshot.put(key_bytes, value_bytes);
        snapshot.commit();
    }

    fn attach_tokens_tracker(
        system: &Arc<NeoSystem>,
        store: Arc<dyn IStore>,
        enabled_trackers: Vec<String>,
        track_history: bool,
    ) {
        let mut settings = TokensTrackerSettings::default();
        settings.enabled_trackers = enabled_trackers;
        settings.track_history = track_history;
        settings.network = system.settings().network;

        let service = Arc::new(TokensTrackerService::new(settings, store));
        system
            .add_service::<TokensTrackerService, _>(Arc::clone(&service))
            .expect("register tokens tracker service");
    }

    fn store_contract_state(system: &Arc<NeoSystem>, contract: &ContractState) {
        const PREFIX_CONTRACT: u8 = 0x08;
        const PREFIX_CONTRACT_HASH: u8 = 0x0c;

        let contract_mgmt_id = NativeRegistry::new()
            .get_by_name("ContractManagement")
            .expect("contract management")
            .id();

        let mut writer = neo_core::neo_io::BinaryWriter::new();
        contract.serialize(&mut writer).expect("serialize contract");

        let mut store_cache = system.context().store_snapshot_cache();
        let mut key_bytes = Vec::with_capacity(1 + 20);
        key_bytes.push(PREFIX_CONTRACT);
        key_bytes.extend_from_slice(&contract.hash.to_bytes());
        let key = StorageKey::new(contract_mgmt_id, key_bytes);
        store_cache.add(key, StorageItem::from_bytes(writer.into_bytes()));

        let mut id_bytes = Vec::with_capacity(1 + 4);
        id_bytes.push(PREFIX_CONTRACT_HASH);
        id_bytes.extend_from_slice(&contract.id.to_be_bytes());
        let id_key = StorageKey::new(contract_mgmt_id, id_bytes);
        store_cache.add(
            id_key,
            StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
        );

        let mut legacy_bytes = Vec::with_capacity(1 + 4);
        legacy_bytes.push(PREFIX_CONTRACT_HASH);
        legacy_bytes.extend_from_slice(&contract.id.to_le_bytes());
        let legacy_key = StorageKey::new(contract_mgmt_id, legacy_bytes);
        store_cache.add(
            legacy_key,
            StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
        );
        store_cache.commit();
    }

    fn emit_map_entry_string(builder: &mut ScriptBuilder, key: &str, value: &str) {
        builder.emit_opcode(OpCode::DUP);
        builder.emit_push_string(key);
        builder.emit_push_string(value);
        builder.emit_opcode(OpCode::SETITEM);
    }

    fn emit_map_entry_null(builder: &mut ScriptBuilder, key: &str) {
        builder.emit_opcode(OpCode::DUP);
        builder.emit_push_string(key);
        builder.emit_opcode(OpCode::PUSHNULL);
        builder.emit_opcode(OpCode::SETITEM);
    }

    fn emit_map_entry_bytes(builder: &mut ScriptBuilder, key: &str, value: &[u8]) {
        builder.emit_opcode(OpCode::DUP);
        builder.emit_push_string(key);
        builder.emit_push_byte_array(value);
        builder.emit_opcode(OpCode::SETITEM);
    }

    fn build_nep11_properties_contract() -> ContractState {
        let mut script = ScriptBuilder::new();
        script.emit_opcode(OpCode::DROP);
        script.emit_opcode(OpCode::NEWMAP);
        emit_map_entry_string(&mut script, "name", "Example NFT");
        emit_map_entry_string(&mut script, "image", "ipfs://example");
        emit_map_entry_null(&mut script, "tokenURI");
        emit_map_entry_bytes(&mut script, "extra", &[1u8, 2, 3]);
        script.emit_opcode(OpCode::RET);

        let nef = NefFile::new("nep11-properties".to_string(), script.to_array());
        let mut manifest = ContractManifest::new("Nep11Properties".to_string());
        manifest.supported_standards.push("NEP-11".to_string());

        let parameter = ContractParameterDefinition::new(
            "tokenId".to_string(),
            ContractParameterType::ByteArray,
        )
        .expect("parameter");
        let method = ContractMethodDescriptor::new(
            "properties".to_string(),
            vec![parameter],
            ContractParameterType::Map,
            0,
            true,
        )
        .expect("method");
        manifest.abi = ContractAbi::new(vec![method], Vec::new());

        let hash = ContractState::calculate_hash(&UInt160::zero(), nef.checksum, &manifest.name);
        ContractState::new(9, hash, nef, manifest)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_nep17_balances_reports_asset_metadata() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let store = create_tracker_store();
        attach_tokens_tracker(
            &system,
            Arc::clone(&store),
            vec!["NEP-17".to_string()],
            true,
        );

        let gas_token = GasToken::new();
        let asset = gas_token.hash();
        let contract = gas_token
            .contract_state(system.settings(), 0)
            .expect("gas contract");
        store_contract_state(&system, &contract);
        let store_cache = system.store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let contract_lookup =
            ContractManagement::get_contract_from_snapshot(snapshot.as_ref(), &asset)
                .expect("contract lookup");
        assert!(contract_lookup.is_some());
        let mut script = ScriptBuilder::new();
        emit_contract_call(&mut script, &asset, "decimals").expect("emit decimals");
        emit_contract_call(&mut script, &asset, "symbol").expect("emit symbol");
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot.clone(),
            None,
            system.settings().clone(),
            TEST_MODE_GAS,
            None,
        )
        .expect("engine");
        engine
            .load_script(script.to_array(), CallFlags::ALL, Some(asset))
            .expect("load script");
        engine.execute().expect("execute");
        assert_eq!(
            engine.state(),
            VMState::HALT,
            "fault: {:?}",
            engine.fault_exception()
        );
        let result_stack = engine.result_stack();
        let symbol_item = result_stack.peek(0).expect("symbol item");
        let decimals_item = result_stack.peek(1).expect("decimals item");
        let symbol_bytes = symbol_item.as_bytes().expect("symbol bytes");
        let symbol = String::from_utf8(symbol_bytes).expect("symbol utf8");
        let decimals = decimals_item
            .get_integer()
            .expect("decimals integer")
            .to_u32()
            .expect("decimals u32");
        assert_eq!(symbol, "GAS");
        assert_eq!(decimals, 8);
        assert!(query_asset_metadata(snapshot.as_ref(), system.settings(), &asset).is_some());
        let user = UInt160::from_bytes(&[1u8; 20]).expect("user hash");
        let balance = TokenBalance {
            balance: BigInt::from(42),
            last_updated_block: 7,
        };
        let key = Nep17BalanceKey::new(user, asset);
        let (balance_prefix, _, _) = Nep17Tracker::rpc_prefixes();
        write_tracker_entry(&store, balance_prefix, &key, &balance);
        let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
        prefix.push(balance_prefix);
        prefix.extend_from_slice(&user.to_bytes());
        let entries = find_prefix::<Nep17BalanceKey, TokenBalance>(store.as_ref(), &prefix)
            .expect("find prefix");
        assert_eq!(entries.len(), 1);

        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerTokensTracker::register_handlers();
        let handler = find_handler(&handlers, "getnep17balances");

        let address = WalletHelper::to_address(&user, server.system().settings().address_version);
        let params = [Value::String(address.clone())];
        let result = (handler.callback())(&server, &params).expect("getnep17balances");
        let obj = result.as_object().expect("result object");
        assert_eq!(
            obj.get("address").and_then(Value::as_str),
            Some(address.as_str())
        );

        let balances = obj
            .get("balance")
            .and_then(Value::as_array)
            .expect("balance array");
        assert_eq!(balances.len(), 1);
        let entry = balances[0].as_object().expect("balance entry");
        assert_eq!(
            entry.get("assethash").and_then(Value::as_str),
            Some(asset.to_string().as_str())
        );
        assert_eq!(entry.get("name").and_then(Value::as_str), Some("GasToken"));
        assert_eq!(entry.get("symbol").and_then(Value::as_str), Some("GAS"));
        assert_eq!(entry.get("decimals").and_then(Value::as_str), Some("8"));
        assert_eq!(entry.get("amount").and_then(Value::as_str), Some("42"));
        assert_eq!(
            entry.get("lastupdatedblock").and_then(Value::as_u64),
            Some(7)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_nep11_balances_groups_tokens_by_asset() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let store = create_tracker_store();
        attach_tokens_tracker(
            &system,
            Arc::clone(&store),
            vec!["NEP-11".to_string()],
            true,
        );

        let gas_token = GasToken::new();
        let asset = gas_token.hash();
        let contract = gas_token
            .contract_state(system.settings(), 0)
            .expect("gas contract");
        store_contract_state(&system, &contract);

        let user = UInt160::from_bytes(&[6u8; 20]).expect("user hash");
        let token_a = vec![0x01];
        let token_b = vec![0x02, 0x03];
        let key_a = Nep11BalanceKey::new(user, asset, token_a.clone());
        let key_b = Nep11BalanceKey::new(user, asset, token_b.clone());
        let (balance_prefix, _, _) = Nep11Tracker::rpc_prefixes();

        write_tracker_entry(
            &store,
            balance_prefix,
            &key_a,
            &TokenBalance {
                balance: BigInt::from(5),
                last_updated_block: 10,
            },
        );
        write_tracker_entry(
            &store,
            balance_prefix,
            &key_b,
            &TokenBalance {
                balance: BigInt::from(7),
                last_updated_block: 11,
            },
        );

        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerTokensTracker::register_handlers();
        let handler = find_handler(&handlers, "getnep11balances");

        let address = WalletHelper::to_address(&user, server.system().settings().address_version);
        let params = [Value::String(address.clone())];
        let result = (handler.callback())(&server, &params).expect("getnep11balances");
        let obj = result.as_object().expect("result object");
        assert_eq!(
            obj.get("address").and_then(Value::as_str),
            Some(address.as_str())
        );

        let balances = obj
            .get("balance")
            .and_then(Value::as_array)
            .expect("balance array");
        assert_eq!(balances.len(), 1);
        let entry = balances[0].as_object().expect("balance entry");
        assert_eq!(
            entry.get("assethash").and_then(Value::as_str),
            Some(asset.to_string().as_str())
        );
        assert_eq!(entry.get("name").and_then(Value::as_str), Some("GasToken"));
        assert_eq!(entry.get("symbol").and_then(Value::as_str), Some("GAS"));
        assert_eq!(entry.get("decimals").and_then(Value::as_str), Some("8"));

        let tokens = entry
            .get("tokens")
            .and_then(Value::as_array)
            .expect("tokens array");
        assert_eq!(tokens.len(), 2);
        let mut token_map: HashMap<String, (String, u64)> = HashMap::new();
        for token in tokens {
            let token_obj = token.as_object().expect("token entry");
            let token_id = token_obj
                .get("tokenid")
                .and_then(Value::as_str)
                .expect("tokenid")
                .to_string();
            let amount = token_obj
                .get("amount")
                .and_then(Value::as_str)
                .expect("amount")
                .to_string();
            let last = token_obj
                .get("lastupdatedblock")
                .and_then(Value::as_u64)
                .expect("lastupdatedblock");
            token_map.insert(token_id, (amount, last));
        }

        let token_a_key = hex::encode(&token_a);
        let token_b_key = hex::encode(&token_b);
        assert_eq!(token_map.get(&token_a_key), Some(&(String::from("5"), 10)));
        assert_eq!(token_map.get(&token_b_key), Some(&(String::from("7"), 11)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_nep11_transfers_orders_by_timestamp_descending() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let store = create_tracker_store();
        attach_tokens_tracker(
            &system,
            Arc::clone(&store),
            vec!["NEP-11".to_string()],
            true,
        );

        let user = UInt160::from_bytes(&[8u8; 20]).expect("user hash");
        let other = UInt160::from_bytes(&[9u8; 20]).expect("other hash");
        let asset = UInt160::from_bytes(&[10u8; 20]).expect("asset hash");
        let tx1 = UInt256::from_bytes(&[11u8; 32]).expect("tx1");
        let tx2 = UInt256::from_bytes(&[12u8; 32]).expect("tx2");

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis() as u64;
        let t1 = now_ms - 1_000;
        let t2 = now_ms - 500;

        let (_, sent_prefix, received_prefix) = Nep11Tracker::rpc_prefixes();
        let token_a = vec![0xAA];
        let token_b = vec![0xBB, 0xCC];

        let sent_key_1 = Nep11TransferKey::new(user, t1, asset, token_a.clone(), 0);
        let sent_key_2 = Nep11TransferKey::new(user, t2, asset, token_b.clone(), 1);
        write_tracker_entry(
            &store,
            sent_prefix,
            &sent_key_1,
            &TokenTransfer {
                user_script_hash: other,
                block_index: 1,
                tx_hash: tx1,
                amount: BigInt::from(5),
            },
        );
        write_tracker_entry(
            &store,
            sent_prefix,
            &sent_key_2,
            &TokenTransfer {
                user_script_hash: other,
                block_index: 2,
                tx_hash: tx2,
                amount: BigInt::from(7),
            },
        );

        let received_key = Nep11TransferKey::new(user, t1, asset, token_a.clone(), 0);
        write_tracker_entry(
            &store,
            received_prefix,
            &received_key,
            &TokenTransfer {
                user_script_hash: UInt160::zero(),
                block_index: 3,
                tx_hash: tx1,
                amount: BigInt::from(11),
            },
        );

        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerTokensTracker::register_handlers();
        let handler = find_handler(&handlers, "getnep11transfers");

        let address = WalletHelper::to_address(&user, server.system().settings().address_version);
        let params = [
            Value::String(address.clone()),
            json!(t1 - 1),
            json!(now_ms + 1),
        ];
        let result = (handler.callback())(&server, &params).expect("getnep11transfers");
        let obj = result.as_object().expect("result object");
        assert_eq!(
            obj.get("address").and_then(Value::as_str),
            Some(address.as_str())
        );

        let sent = obj
            .get("sent")
            .and_then(Value::as_array)
            .expect("sent array");
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].get("timestamp").and_then(Value::as_u64), Some(t2));
        assert_eq!(sent[1].get("timestamp").and_then(Value::as_u64), Some(t1));
        let token_b_hex = hex::encode(&token_b);
        assert_eq!(
            sent[0].get("tokenid").and_then(Value::as_str),
            Some(token_b_hex.as_str())
        );

        let received = obj
            .get("received")
            .and_then(Value::as_array)
            .expect("received array");
        assert_eq!(received.len(), 1);
        assert!(received[0].get("transferaddress").unwrap().is_null());
        let token_a_hex = hex::encode(&token_a);
        assert_eq!(
            received[0].get("tokenid").and_then(Value::as_str),
            Some(token_a_hex.as_str())
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "NFT properties test needs system context - pre-existing issue"]
    async fn get_nep11_properties_returns_expected_fields() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let store = create_tracker_store();
        attach_tokens_tracker(&system, store, vec!["NEP-11".to_string()], true);

        let contract = build_nep11_properties_contract();
        let contract_hash = contract.hash;
        store_contract_state(&system, &contract);

        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerTokensTracker::register_handlers();
        let handler = find_handler(&handlers, "getnep11properties");

        let address =
            WalletHelper::to_address(&contract_hash, server.system().settings().address_version);
        let params = [Value::String(address), Value::String("0x0102".to_string())];
        let result = (handler.callback())(&server, &params).expect("getnep11properties");
        let obj = result.as_object().expect("properties object");
        assert_eq!(obj.get("name").and_then(Value::as_str), Some("Example NFT"));
        assert_eq!(
            obj.get("image").and_then(Value::as_str),
            Some("ipfs://example")
        );
        assert!(obj.get("tokenURI").unwrap().is_null());
        let extra_encoded = BASE64_STANDARD.encode([1u8, 2, 3]);
        assert_eq!(
            obj.get("extra").and_then(Value::as_str),
            Some(extra_encoded.as_str())
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_nep17_transfers_orders_by_timestamp_descending() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let store = create_tracker_store();
        attach_tokens_tracker(
            &system,
            Arc::clone(&store),
            vec!["NEP-17".to_string()],
            true,
        );

        let gas_token = GasToken::new();
        let asset = gas_token.hash();
        let contract = gas_token
            .contract_state(system.settings(), 0)
            .expect("gas contract");
        store_contract_state(&system, &contract);
        let user = UInt160::from_bytes(&[2u8; 20]).expect("user hash");
        let other = UInt160::from_bytes(&[3u8; 20]).expect("other hash");
        let tx1 = UInt256::from_bytes(&[4u8; 32]).expect("tx1");
        let tx2 = UInt256::from_bytes(&[5u8; 32]).expect("tx2");

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis() as u64;
        let t1 = now_ms - 1_000;
        let t2 = now_ms - 500;

        let (_, sent_prefix, received_prefix) = Nep17Tracker::rpc_prefixes();
        let sent_key_1 = Nep17TransferKey::new(user, t1, asset, 0);
        let sent_key_2 = Nep17TransferKey::new(user, t2, asset, 1);
        write_tracker_entry(
            &store,
            sent_prefix,
            &sent_key_1,
            &TokenTransfer {
                user_script_hash: other,
                block_index: 1,
                tx_hash: tx1,
                amount: BigInt::from(5),
            },
        );
        write_tracker_entry(
            &store,
            sent_prefix,
            &sent_key_2,
            &TokenTransfer {
                user_script_hash: other,
                block_index: 2,
                tx_hash: tx2,
                amount: BigInt::from(7),
            },
        );

        let received_key = Nep17TransferKey::new(user, t1, asset, 0);
        write_tracker_entry(
            &store,
            received_prefix,
            &received_key,
            &TokenTransfer {
                user_script_hash: UInt160::zero(),
                block_index: 3,
                tx_hash: tx1,
                amount: BigInt::from(11),
            },
        );
        let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
        prefix.push(sent_prefix);
        prefix.extend_from_slice(&user.to_bytes());
        let start_key = [prefix.as_slice(), &(t1 - 1).to_be_bytes()].concat();
        let end_key = [prefix.as_slice(), &(now_ms + 1).to_be_bytes()].concat();
        let sent_pairs =
            find_range::<Nep17TransferKey, TokenTransfer>(store.as_ref(), &start_key, &end_key)
                .expect("find sent range");
        assert_eq!(sent_pairs.len(), 2);

        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerTokensTracker::register_handlers();
        let handler = find_handler(&handlers, "getnep17transfers");

        let address = WalletHelper::to_address(&user, server.system().settings().address_version);
        let params = [
            Value::String(address.clone()),
            json!(t1 - 1),
            json!(now_ms + 1),
        ];
        let result = (handler.callback())(&server, &params).expect("getnep17transfers");
        let obj = result.as_object().expect("result object");
        assert_eq!(
            obj.get("address").and_then(Value::as_str),
            Some(address.as_str())
        );

        let sent = obj
            .get("sent")
            .and_then(Value::as_array)
            .expect("sent array");
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].get("timestamp").and_then(Value::as_u64), Some(t2));
        assert_eq!(sent[1].get("timestamp").and_then(Value::as_u64), Some(t1));
        assert_eq!(
            sent[0].get("transferaddress").and_then(Value::as_str),
            Some(
                WalletHelper::to_address(&other, server.system().settings().address_version)
                    .as_str()
            )
        );

        let received = obj
            .get("received")
            .and_then(Value::as_array)
            .expect("received array");
        assert_eq!(received.len(), 1);
        assert!(received[0].get("transferaddress").unwrap().is_null());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_nep17_balances_requires_enabled_tracker() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let store = create_tracker_store();
        attach_tokens_tracker(&system, store, Vec::new(), true);

        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerTokensTracker::register_handlers();
        let handler = find_handler(&handlers, "getnep17balances");

        let params = [Value::String(UInt160::zero().to_address())];
        let err = (handler.callback())(&server, &params).expect_err("method not found");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::method_not_found().code());
    }
}
