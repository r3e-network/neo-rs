//! Contract invocation endpoints (`invokefunction` / `invokescript`).
//!
//! Both endpoints are *read-only simulation*: the script is executed against a
//! throwaway snapshot and only the VM result is reported. They never construct
//! a transaction, never call `WalletAccount::create_witness`, and never consult
//! the node wallet, so an open wallet cannot be turned into a signing oracle
//! that bypasses the unified signer service.
//!
//! Signing and broadcasting belong to the wallet RPC endpoints
//! (`sendtoaddress`, `sendfrom`, `sendmany`, `canceltransaction`, ...), which
//! route every signature through the signer runtime so policy, provider
//! selection, quota and durable-operation controls all apply. A caller that
//! wants to invoke *and* sign builds the transaction locally (from the
//! simulation result and the `calculatenetworkfee` response) and submits it
//! through `sendrawtransaction`.

use std::str::FromStr;

use crate::rpc_helpers::expect_base64_param_with_decode_message;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_primitives::UInt160;
use neo_protocol::payloads::Signer;
use neo_protocol::witness::Witness;
use serde_json::{Map, Value, json};

use crate::diagnostic::Diagnostic;
use crate::rpc_exception::RpcException;
use crate::rpc_server::RpcServer;
use crate::session::Session;
use neo_vm::StackItem;

use super::helpers::{
    DEFAULT_RESPONSE_BUDGET_BYTES, build_dynamic_call_script, diagnostic_invocation_to_json,
    diagnostic_storage_changes, expect_string_param, final_rpc_vm_state_string, internal_error,
    invalid_params, notification_to_json, parse_contract_parameters, parse_signers_and_witnesses,
    stack_item_to_json,
};

/// Runs a dynamically built contract call for simulation only.
///
/// The optional `signers`/`witnesses` are used solely to give the simulated
/// engine a `Runtime.CheckWitness` context; nothing is signed, and the result
/// carries no transaction or signature context.
pub(super) fn invoke_function(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
    let _span = tracing::info_span!("rpc_invoke_function").entered();

    let script_hash = expect_string_param(params, 0, "invokefunction")?;
    let script_hash = UInt160::from_str(&script_hash)
        .map_err(|err| invalid_params(format!("invalid script hash: {err}")))?;

    let operation = expect_string_param(params, 1, "invokefunction")?;

    tracing::debug!(
        contract = %script_hash,
        operation = %operation,
        "Invoking contract function"
    );

    let parameters = parse_contract_parameters(params.get(2))?;
    let (signers, witnesses) = parse_signers_and_witnesses(server, params.get(3))?;
    let use_diagnostic = params
        .get(4)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let script = build_dynamic_call_script(script_hash, &operation, &parameters)?;
    execute_script(server, script, signers, witnesses, use_diagnostic)
}

/// Runs a caller-supplied script for simulation only.
///
/// See [`invoke_function`]: the optional signers/witnesses only shape the
/// simulated witness context and never produce a signature.
pub(super) fn invoke_script(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
    let _span = tracing::info_span!("rpc_invoke_script").entered();

    let script = expect_base64_param_with_decode_message(
        params,
        0,
        "invokescript",
        "invalid script payload",
    )?;

    tracing::debug!(script_length = script.len(), "Invoking script");

    let (signers, witnesses) = parse_signers_and_witnesses(server, params.get(1))?;
    let use_diagnostic = params
        .get(2)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    execute_script(server, script, signers, witnesses, use_diagnostic)
}

/// Executes `script` and renders the VM result.
///
/// # Security
/// This is the only execution path shared by the invocation endpoints and it
/// is deliberately wallet-free: no transaction is built, no witness is
/// produced and `RpcServer::wallet` is never read. Signing lives exclusively in
/// the wallet RPC endpoints behind the unified signer service.
#[tracing::instrument(name = "rpc_execute_script", skip(server, script, signers, witnesses), fields(script_len = script.len(), diagnostic = use_diagnostic))]
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
    let mut session = Session::new(
        server.system(),
        script,
        signers,
        witnesses,
        server.settings().max_gas_invoke,
        diagnostic,
    )
    .map_err(internal_error)?;

    let (
        engine_state,
        system_fee,
        exception_value,
        notifications_snapshot,
        stack_snapshot,
        diagnostics_snapshot,
    ) = {
        let engine = session.engine();
        let engine_state = final_rpc_vm_state_string(engine.state())?;
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

    // R-4: notifications and the result stack share one response budget so a
    // single invoke cannot materialise an unbounded JSON tree.
    let mut budget = DEFAULT_RESPONSE_BUDGET_BYTES;

    let notifications = {
        let mut session_ref = Some(&mut session);
        let mut entries = Vec::new();
        for notification in &notifications_snapshot {
            entries.push(notification_to_json(
                notification,
                session_ref.as_deref_mut(),
                &mut budget,
            )?);
        }
        entries
    };
    result.insert("notifications".to_string(), Value::Array(notifications));

    let stack_items = {
        let mut session_ref = Some(&mut session);
        let mut entries = Vec::new();
        for item in &stack_snapshot {
            match stack_item_to_json(item, session_ref.as_deref_mut(), &mut budget) {
                Ok(value) => entries.push(value),
                Err(err) => {
                    let budget_exhausted = budget == 0;
                    entries.push(Value::String(format!("error: {err}")));
                    if budget_exhausted {
                        // The budget covers the whole response: stop instead of
                        // attempting conversions that can only fail.
                        break;
                    }
                }
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
                "storagechanges": storage,
            }),
        );
    }

    // No `tx` / `pendingsignature` key is ever produced here. The previous
    // implementation built a transaction, signed it with
    // `WalletAccount::create_witness`, and reported an empty pending list as
    // "signed and complete" — even when the signing preimage could not be
    // derived. Removing the signing path removes that bypass and its failure
    // mode together.

    if server.session_enabled() && session.has_iterators() {
        server.purge_expired_sessions();
        let session_id = server.store_session(session).map_err(RpcException::from)?;
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
