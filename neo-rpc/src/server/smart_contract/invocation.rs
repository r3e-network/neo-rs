use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_payloads::signer::Signer;
use neo_payloads::witness::Witness;

use serde_json::{Map, Value, json};

use crate::server::diagnostic::Diagnostic;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use crate::server::session::Session;
use neo_vm::stack_item::StackItem;
use neo_vm_rs::VmState as VMState;

use super::diagnostics::{diagnostic_invocation_to_json, diagnostic_storage_changes};
use super::helpers::internal_error;
use super::invocation_wallet::process_invoke_with_wallet;
use super::request::{InvokeFunctionRequest, InvokeScriptRequest};
use super::response::{final_rpc_vm_state_string, notification_to_json, stack_item_to_json};
use super::script::build_dynamic_call_script;

pub(super) fn invoke_function(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
    let request = InvokeFunctionRequest::parse(server, params)?;
    let script =
        build_dynamic_call_script(request.script_hash, &request.operation, &request.parameters)?;
    execute_script(
        server,
        script,
        request.signers,
        request.witnesses,
        request.use_diagnostic,
    )
}

pub(super) fn invoke_script(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
    let request = InvokeScriptRequest::parse(server, params)?;
    execute_script(
        server,
        request.script,
        request.signers,
        request.witnesses,
        request.use_diagnostic,
    )
}

fn execute_script(
    server: &RpcServer,
    script: Vec<u8>,
    signers: Option<Vec<Signer>>,
    witnesses: Option<Vec<Witness>>,
    use_diagnostic: bool,
) -> Result<Value, RpcException> {
    let diagnostic = if use_diagnostic {
        Some(Diagnostic::new())
    } else {
        None
    };
    let system = server.system();
    let mut session = Session::new(
        system.clone(), // Arc<NodeContext> coerced to Arc<dyn StoreProvider>
        system.clone(), // Arc<NodeContext> coerced to Arc<dyn ConfigProvider>
        system.native_contract_provider(),
        script,
        signers.clone(),
        witnesses,
        server.settings().max_gas_invoke,
        diagnostic,
    )
    .map_err(internal_error)?;

    let (
        vm_state,
        engine_state,
        system_fee,
        exception_value,
        notifications_snapshot,
        stack_snapshot,
        diagnostics_snapshot,
    ) = {
        let engine = session.engine();
        let vm_state = engine.state();
        let engine_state = final_rpc_vm_state_string(vm_state)?;
        let system_fee = engine.fee_consumed();
        let exception_value = engine.fault_exception().map_or(Value::Null, |msg| {
            Value::String(normalize_fault_message(msg))
        });
        let notifications_snapshot = engine.notifications().to_vec();
        let stack_snapshot: Vec<StackItem> = engine.result_stack().iter().cloned().collect();
        let diagnostics_snapshot = session.diagnostic().map(|diag| {
            let invocation = diagnostic_invocation_to_json(&diag);
            let storage = diagnostic_storage_changes(&engine);
            (invocation, storage)
        });
        (
            vm_state,
            engine_state,
            system_fee,
            exception_value,
            notifications_snapshot,
            stack_snapshot,
            diagnostics_snapshot,
        )
    };
    let gas_consumed = system_fee.to_string();

    let mut result = Map::new();
    result.insert(
        "script".to_string(),
        Value::String(BASE64_STANDARD.encode(session.script())),
    );
    result.insert("state".to_string(), Value::String(engine_state));
    result.insert("gasconsumed".to_string(), Value::String(gas_consumed));
    result.insert("exception".to_string(), exception_value);

    let notifications = {
        let mut session_ref = Some(&mut session);
        let mut entries = Vec::new();
        for notification in &notifications_snapshot {
            entries.push(notification_to_json(
                notification,
                session_ref.as_deref_mut(),
            )?);
        }
        entries
    };
    result.insert("notifications".to_string(), Value::Array(notifications));

    let stack_items = {
        let mut session_ref = Some(&mut session);
        let mut entries = Vec::new();
        for item in &stack_snapshot {
            match stack_item_to_json(item, session_ref.as_deref_mut()) {
                Ok(value) => entries.push(value),
                Err(err) => entries.push(Value::String(format!("error: {err}"))),
            }
        }
        entries
    };
    result.insert("stack".to_string(), Value::Array(stack_items));

    if let Some((invocation, storage)) = diagnostics_snapshot {
        result.insert(
            "diagnostics".to_string(),
            json!({
                "invokedcontracts": invocation,
                "storagechanges": storage}),
        );
    }

    if vm_state != VMState::FAULT {
        process_invoke_with_wallet(
            server,
            &mut result,
            session.script(),
            signers.as_deref(),
            session.snapshot(),
            system_fee,
        );
    }

    if server.session_enabled() && session.has_iterators() {
        server.purge_expired_sessions();
        let session_id = server.store_session(session);
        result.insert("session".to_string(), Value::String(session_id.to_string()));
    }

    Ok(Value::Object(result))
}

fn normalize_fault_message(message: &str) -> String {
    if message.contains("Gas exhausted") || message.contains("Gas limit exceeded") {
        "Insufficient GAS".to_string()
    } else {
        message.to_string()
    }
}
