//! ApplicationEngine probes used by RPC wallet compatibility helpers.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_manifest::CallFlags;
use neo_native_contracts::GasToken;
use neo_primitives::{TriggerType, UInt160, Verifiable};
use neo_storage::persistence::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::{OpCode, VmState as VMState};
use num_bigint::BigInt;

use super::{WalletCompatError, WalletCompatResult};

pub(super) fn run_test_invocation(
    script: Vec<u8>,
    snapshot: &DataCache,
    container: Option<Arc<dyn Verifiable>>,
    settings: &ProtocolSettings,
    max_gas: i64,
) -> CoreResult<ApplicationEngine> {
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        container,
        Arc::new(snapshot.clone()),
        None,
        settings.clone(),
        max_gas,
        None,
    )
    .map_err(|err| CoreError::other(err.to_string()))?;
    engine
        .load_script(script, CallFlags::ALL, None)
        .map_err(|err| CoreError::other(err.to_string()))?;
    engine.execute_allow_fault();
    Ok(engine)
}

/// `NativeContract.GAS.BalanceOf(snapshot, account)` via a `balanceOf`
/// engine probe (the canonical read in the Rust tree).
pub(crate) fn gas_balance_of(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    account: &UInt160,
) -> WalletCompatResult<BigInt> {
    nep17_balance_of(snapshot, settings, &GasToken::script_hash(), account)
}

/// `balanceOf` probe for an arbitrary NEP-17 asset.
pub(super) fn nep17_balance_of(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    asset: &UInt160,
    account: &UInt160,
) -> WalletCompatResult<BigInt> {
    let mut builder = ScriptBuilder::new();
    emit_dynamic_call(
        &mut builder,
        asset,
        "balanceOf",
        &[CallArg::Bytes(account.to_bytes())],
    )
    .map_err(|e| WalletCompatError::Other(e.to_string()))?;
    let engine = run_test_invocation(
        builder.to_array(),
        snapshot,
        None,
        settings,
        BALANCE_PROBE_GAS,
    )
    .map_err(|e| WalletCompatError::Other(e.to_string()))?;
    if engine.state() != VMState::HALT {
        return Err(WalletCompatError::Other(format!(
            "Failed to execute balanceOf method for asset {asset} on account {account}. The \
             smart contract execution faulted with state: {:?}.",
            engine.state()
        )));
    }
    engine
        .result_stack()
        .peek(0)
        .map_err(|err| WalletCompatError::Other(err.to_string()))?
        .as_int()
        .map_err(|err| WalletCompatError::Other(err.to_string()))
}

/// GAS budget for `balanceOf` probes — C# uses the test-mode default
/// (`ApplicationEngine.TestModeGas`, 2 GAS in datoshi).
const BALANCE_PROBE_GAS: i64 = 2_0000_0000;

/// Argument for [`emit_dynamic_call`].
pub(super) enum CallArg {
    Bytes(Vec<u8>),
    Int(BigInt),
    Null,
}

/// `ScriptBuilderExtensions.EmitDynamicCall(hash, method, args…)` with
/// `CallFlags::ALL` (the C# default used by transfer scripts).
pub(super) fn emit_dynamic_call(
    builder: &mut ScriptBuilder,
    contract: &UInt160,
    method: &str,
    args: &[CallArg],
) -> CoreResult<()> {
    if args.is_empty() {
        builder.emit_push_int(0);
        builder.emit_pack();
    } else {
        for arg in args.iter().rev() {
            match arg {
                CallArg::Bytes(bytes) => {
                    builder.emit_push(bytes);
                }
                CallArg::Int(value) => {
                    builder
                        .emit_push_bigint(value.clone())
                        .map_err(|err| CoreError::other(err.to_string()))?;
                }
                CallArg::Null => {
                    builder.emit_opcode(OpCode::PUSHNULL);
                }
            }
        }
        builder.emit_push_int(args.len() as i64);
        builder.emit_pack();
    }
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(method.as_bytes());
    builder.emit_push(&contract.to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| CoreError::other(err.to_string()))?;
    Ok(())
}
