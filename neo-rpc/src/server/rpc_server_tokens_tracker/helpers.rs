use crate::plugins::tokens_tracker::trackers::tracker_base::TokenTransferKeyView;
use crate::plugins::tokens_tracker::{
    Nep11TransferKey, Nep17TransferKey, TokenTransfer, TokensTrackerService, find_range,
};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use neo_execution::application_engine::TEST_MODE_GAS;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_io::Serializable;
use neo_manifest::CallFlags;
use neo_primitives::UInt160;
use neo_storage::CacheRead;
use neo_storage::persistence::providers::RuntimeStore;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use num_traits::ToPrimitive;
use serde_json::Value;
use std::sync::Arc;

use super::response::{nep11_transfer_entry, transfer_entries, transfer_entry};

pub(super) fn tracker_service(
    server: &RpcServer,
) -> Result<Arc<TokensTrackerService<RuntimeStore>>, RpcException> {
    server
        .system()
        .tokens_tracker_service()
        .ok_or_else(|| internal_error("TokensTracker service not available"))
}

pub(super) fn collect_transfers(
    store: &impl neo_storage::persistence::Store,
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
        max_results,
        |key, value| transfer_entry(key, value, address_version),
    )
}

pub(super) fn collect_nep11_transfers(
    store: &impl neo_storage::persistence::Store,
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
        max_results,
        |key, value| nep11_transfer_entry(key, value, address_version),
    )
}

fn collect_transfer_entries<K, S, F>(
    store: &S,
    prefix: u8,
    script_hash: &UInt160,
    start: u64,
    end: u64,
    max_results: usize,
    project_entry: F,
) -> Result<Value, RpcException>
where
    K: Serializable + TokenTransferKeyView,
    S: neo_storage::persistence::Store,
    F: Fn(&K, &TokenTransfer) -> Value,
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
        entries.push(project_entry(&key, &value));
    }

    Ok(transfer_entries(entries))
}

pub(super) fn query_asset_metadata<P, B>(
    snapshot: &neo_storage::persistence::DataCache<B>,
    settings: &neo_config::ProtocolSettings,
    native_contract_provider: Arc<P>,
    asset: &UInt160,
) -> Option<(String, u32)>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
{
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
            neo_execution::NoDiagnostic,
            native_contract_provider,
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
