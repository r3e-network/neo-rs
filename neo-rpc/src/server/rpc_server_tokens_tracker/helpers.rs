use crate::plugins::tokens_tracker::trackers::tracker_base::TokenTransferKeyView;
use crate::plugins::tokens_tracker::{
    Nep11TransferKey, Nep17TransferKey, TokenTransfer, TokensTrackerService, find_range,
};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use neo_execution::application_engine::TEST_MODE_GAS;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_io::Serializable;
use neo_manifest::CallFlags;
use neo_primitives::UInt160;
use neo_primitives::hex_util;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use num_traits::ToPrimitive;
use serde_json::{Map, Value, json};
use std::sync::Arc;

use super::RpcServer;

pub(super) fn tracker_service(
    server: &RpcServer,
) -> Result<Arc<TokensTrackerService>, RpcException> {
    server
        .system()
        .get_service::<TokensTrackerService>()
        .ok_or_else(|| internal_error("TokensTracker service not available"))
}

pub(super) fn collect_transfers(
    store: &(impl neo_storage::persistence::Store + ?Sized),
    prefix: u8,
    script_hash: &UInt160,
    start: u64,
    end: u64,
    address_version: u8,
    max_results: usize,
) -> Result<Value, RpcException> {
    collect_transfer_entries::<Nep17TransferKey, _, _>(
        store,
        prefix,
        script_hash,
        start,
        end,
        address_version,
        max_results,
        |_, _| {},
    )
}

pub(super) fn collect_nep11_transfers(
    store: &(impl neo_storage::persistence::Store + ?Sized),
    prefix: u8,
    script_hash: &UInt160,
    start: u64,
    end: u64,
    address_version: u8,
    max_results: usize,
) -> Result<Value, RpcException> {
    collect_transfer_entries::<Nep11TransferKey, _, _>(
        store,
        prefix,
        script_hash,
        start,
        end,
        address_version,
        max_results,
        |key, entry| {
            entry.insert(
                "tokenid".to_string(),
                Value::String(hex_util::encode_hex(&key.token)),
            );
        },
    )
}

fn collect_transfer_entries<K, S, F>(
    store: &S,
    prefix: u8,
    script_hash: &UInt160,
    start: u64,
    end: u64,
    address_version: u8,
    max_results: usize,
    add_extra_fields: F,
) -> Result<Value, RpcException>
where
    K: Serializable + TokenTransferKeyView,
    S: neo_storage::persistence::Store + ?Sized,
    F: Fn(&K, &mut Map<String, Value>),
{
    let mut prefix_bytes = Vec::with_capacity(1 + UInt160::LENGTH);
    prefix_bytes.push(prefix);
    prefix_bytes.extend_from_slice(&script_hash.to_bytes());

    let start_key = [prefix_bytes.as_slice(), &start.to_be_bytes()].concat();
    let end_key = [prefix_bytes.as_slice(), &end.to_be_bytes()].concat();

    let pairs =
        find_range::<_, K, TokenTransfer>(store, &start_key, &end_key).map_err(internal_error)?;

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
        let mut entry = transfer_entry_to_json(&key, &value, address_version);
        add_extra_fields(&key, &mut entry);
        entries.push(Value::Object(entry));
    }

    Ok(Value::Array(entries))
}

fn transfer_entry_to_json<K>(
    key: &K,
    value: &TokenTransfer,
    address_version: u8,
) -> Map<String, Value>
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

    let mut entry = Map::new();
    entry.insert("timestamp".to_string(), json!(key.timestamp_ms()));
    entry.insert(
        "assethash".to_string(),
        Value::String(key.asset_script_hash().to_string()),
    );
    entry.insert("transferaddress".to_string(), transfer_address);
    entry.insert(
        "amount".to_string(),
        Value::String(value.amount.to_string()),
    );
    entry.insert("blockindex".to_string(), json!(value.block_index));
    entry.insert(
        "transfernotifyindex".to_string(),
        json!(key.block_xfer_notification_index()),
    );
    entry.insert(
        "txhash".to_string(),
        Value::String(value.tx_hash.to_string()),
    );
    entry
}

pub(super) fn query_asset_metadata(
    snapshot: &neo_storage::persistence::DataCache,
    settings: &neo_config::ProtocolSettings,
    native_contract_provider: Arc<dyn NativeContractProvider>,
    asset: &UInt160,
) -> Option<(String, u32)> {
    let mut script = ScriptBuilder::new();
    emit_contract_call(&mut script, asset, "decimals").ok()?;
    emit_contract_call(&mut script, asset, "symbol").ok()?;

    let mut engine =
        neo_execution::ApplicationEngine::new_with_shared_block_and_native_contract_provider(
            neo_primitives::TriggerType::Application,
            None,
            Arc::new(snapshot.clone()),
            None,
            settings.clone(),
            TEST_MODE_GAS,
            None,
            Some(native_contract_provider),
        )
        .ok()?;
    engine
        .load_script(script.to_array(), CallFlags::ALL, Some(*asset))
        .ok()?;
    engine.execute().ok()?;
    if engine.state() != VMState::HALT {
        return None;
    }

    let result_stack = engine.result_stack();
    let symbol_item = result_stack.peek(0).ok()?;
    let decimals_item = result_stack.peek(1).ok()?;

    let symbol_bytes = symbol_item.as_bytes().ok()?;
    let symbol = String::from_utf8(symbol_bytes).ok()?;
    let decimals = decimals_item.as_integer().ok()?.to_u32()?;

    Some((symbol, decimals))
}

pub(super) fn emit_contract_call(
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

pub(super) fn emit_contract_call_with_arg(
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
