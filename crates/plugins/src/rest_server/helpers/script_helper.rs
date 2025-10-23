//! Rust port of `Neo.Plugins.RestServer.Helpers.ScriptHelper`.
//!
//! Provides helpers for invoking smart contract methods and arbitrary scripts
//! in the same fashion as the reference C# plugin. The helpers intentionally
//! mirror the control-flow and error handling from the original code so that
//! higher layers can be ported without behavioural divergence.

use crate::rest_server::rest_server_plugin::RestServerGlobals;
use crate::rest_server::rest_server_settings::RestServerSettings;
use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::Signer;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::neo_system::ProtocolSettings;
use neo_core::persistence::data_cache::DataCache;
use neo_core::smart_contract::native::ledger_contract::LedgerContract;
use neo_core::smart_contract::contract_parameter::ContractParameter;
use neo_core::smart_contract::{ApplicationEngine, CallFlags, TriggerType};
use neo_core::UInt160;
use neo_vm::error::VmError;
use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::stack_item::StackItem;
use neo_vm::vm_state::VMState;
use std::sync::Arc;
use thiserror::Error;

/// Errors surfaced by the script helper utilities.
#[derive(Debug, Error)]
pub enum ScriptHelperError {
    #[error("NeoSystem is not initialised")]
    NeoSystemUnavailable,
    #[error("failed to build script: {0}")]
    ScriptBuild(String),
    #[error("application engine error: {0}")]
    Engine(String),
    #[error("invalid contract parameter: {0}")]
    ContractParameter(String),
}

pub struct ScriptHelper;

impl ScriptHelper {
    /// Invokes a read-only method on a smart contract, returning whether the VM
    /// halted successfully alongside the result stack.
    pub fn invoke_method(
        protocol_settings: &ProtocolSettings,
        snapshot: Arc<DataCache>,
        script_hash: &UInt160,
        method: &str,
        args: &[StackItem],
    ) -> Result<(bool, Vec<StackItem>), ScriptHelperError> {
        let outcome = Self::invoke_method_internal(
            protocol_settings,
            snapshot,
            script_hash,
            method,
            CallFlags::READ_ONLY,
            args,
        )?;

        Ok((outcome.halted, outcome.stack))
    }

    /// Invokes a contract method allowing the caller to specify explicit
    /// arguments and signers. Returns the configured application engine so the
    /// caller can inspect notifications or other execution state.
    pub fn invoke_method_with_signers(
        protocol_settings: &ProtocolSettings,
        snapshot: Arc<DataCache>,
        script_hash: &UInt160,
        method: &str,
        args: &[ContractParameter],
        signers: Option<Vec<Signer>>,
    ) -> Result<ApplicationEngine, ScriptHelperError> {
        let stack_args = args
            .iter()
            .map(|param| {
                RestServerUtility::contract_parameter_to_stack_item(param)
                    .map_err(ScriptHelperError::ContractParameter)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let script =
            build_dynamic_call_script(script_hash, method, CallFlags::ALL, &stack_args)?;
        let gas_limit = RestServerSettings::current().max_gas_invoke;

        let mut tx = signers.clone().map(|signer_list| {
            let mut transaction = Transaction::new();
            transaction.set_version(0);
            transaction.set_valid_until_block(
                LedgerContract::new()
                    .current_index(snapshot.as_ref())
                    .unwrap_or(0)
                    .saturating_add(protocol_settings.max_valid_until_block_increment),
            );
            transaction.set_signers(signer_list);
            transaction.set_script(script.clone());
            let witness_count = transaction.signers().len();
            if witness_count > 0 {
                transaction.set_witnesses(vec![Witness::new(); witness_count]);
            }
            transaction
        });

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            tx.as_ref().map(|transaction| Arc::new(transaction.clone()) as Arc<_>),
            Arc::clone(&snapshot),
            None,
            protocol_settings.clone(),
            gas_limit,
            None,
        )
        .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;

        if let Some(transaction) = tx.take() {
            engine
                .load_script(transaction.script().to_vec(), CallFlags::ALL, Some(*script_hash))
                .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;
        } else {
            engine
                .load_script(script, CallFlags::ALL, Some(*script_hash))
                .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;
        }

        engine
            .execute()
            .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;

        Ok(engine)
    }

    /// Invokes a read-only method and returns the execution engine alongside the result stack.
    /// Callers can use this to interact with iterator handles before the engine is dropped.
    pub fn invoke_method_with_engine(
        protocol_settings: &ProtocolSettings,
        snapshot: Arc<DataCache>,
        script_hash: &UInt160,
        method: &str,
        args: &[StackItem],
    ) -> Result<(ApplicationEngine, Vec<StackItem>, bool), ScriptHelperError> {
        let outcome = Self::invoke_method_internal(
            protocol_settings,
            snapshot,
            script_hash,
            method,
            CallFlags::READ_ONLY,
            args,
        )?;

        Ok((outcome.engine, outcome.stack, outcome.halted))
    }

    /// Invokes an arbitrary script against the live node snapshot. Mirrors the
    /// `InvokeScript` helper from the C# plugin.
    pub fn invoke_script(
        script: &[u8],
        signers: Option<Vec<Signer>>,
        witnesses: Option<Vec<Witness>>,
    ) -> Result<ApplicationEngine, ScriptHelperError> {
        let neo_system = RestServerGlobals::neo_system()
            .ok_or(ScriptHelperError::NeoSystemUnavailable)?;
        let snapshot = Arc::new(neo_system.store_cache().data_cache().clone());
        let gas_limit = RestServerSettings::current().max_gas_invoke;
        let system_settings = neo_system.settings();
        let protocol_settings = (*system_settings).clone();

        let mut tx = signers.clone().map(|signer_list| {
            let mut transaction = Transaction::new();
            transaction.set_version(0);
            transaction.set_valid_until_block(
                LedgerContract::new()
                    .current_index(snapshot.as_ref())
                    .unwrap_or(0)
                    .saturating_add(protocol_settings.max_valid_until_block_increment),
            );
            transaction.set_signers(signer_list);
            transaction.set_script(script.to_vec());
            if let Some(mut provided_witnesses) = witnesses.clone() {
                if provided_witnesses.len() != transaction.signers().len() {
                    provided_witnesses.resize(transaction.signers().len(), Witness::new());
                }
                transaction.set_witnesses(provided_witnesses);
            } else if !transaction.signers().is_empty() {
                transaction.set_witnesses(vec![Witness::new(); transaction.signers().len()]);
            }
            transaction
        });

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            tx.as_ref().map(|transaction| Arc::new(transaction.clone()) as Arc<_>),
            Arc::clone(&snapshot),
            None,
            protocol_settings,
            gas_limit,
            None,
        )
        .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;

        if let Some(transaction) = tx.take() {
            engine
                .load_script(transaction.script().to_vec(), CallFlags::ALL, None)
                .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;
        } else {
            engine
                .load_script(script.to_vec(), CallFlags::ALL, None)
                .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;
        }

        engine
            .execute()
            .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;

        Ok(engine)
    }
}

struct InvocationOutcome {
    engine: ApplicationEngine,
    stack: Vec<StackItem>,
    halted: bool,
}

impl ScriptHelper {
    fn invoke_method_internal(
        protocol_settings: &ProtocolSettings,
        snapshot: Arc<DataCache>,
        script_hash: &UInt160,
        method: &str,
        flags: CallFlags,
        args: &[StackItem],
    ) -> Result<InvocationOutcome, ScriptHelperError> {
        let script = build_dynamic_call_script(script_hash, method, flags, args)?;
        let gas_limit = RestServerSettings::current().max_gas_invoke;

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            protocol_settings.clone(),
            gas_limit,
            None,
        )
        .map_err(|err| ScriptHelperError::Engine(err.to_string()))?;

        if engine
            .load_script(script, CallFlags::ALL, Some(*script_hash))
            .is_err()
        {
            return Ok(InvocationOutcome {
                engine,
                stack: Vec::new(),
                halted: false,
            });
        }

        if engine.execute().is_err() {
            let outcome = InvocationOutcome {
                engine,
                stack: Vec::new(),
                halted: false,
            };
            return Ok(outcome);
        }

        let halted = engine.state() == VMState::HALT;
        let stack = if halted {
            engine.result_stack().to_vec()
        } else {
            Vec::new()
        };

        Ok(InvocationOutcome {
            engine,
            stack,
            halted,
        })
    }
}

fn build_dynamic_call_script(
    script_hash: &UInt160,
    method: &str,
    flags: CallFlags,
    args: &[StackItem],
) -> Result<Vec<u8>, ScriptHelperError> {
    let mut builder = ScriptBuilder::new();

    if args.is_empty() {
        builder.emit_opcode(OpCode::NEWARRAY0);
    } else {
        for item in args.iter().rev() {
            emit_stack_item(&mut builder, item.clone())?;
        }
        builder
            .emit_push_int(args.len() as i64)
            .emit_opcode(OpCode::PACK);
    }

    builder.emit_push_int(flags.bits() as i64);
    builder.emit_push(method.as_bytes());
    builder.emit_push(script_hash.to_bytes().as_ref());

    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| ScriptHelperError::ScriptBuild(err.to_string()))?;

    Ok(builder.to_array())
}

fn emit_stack_item(builder: &mut ScriptBuilder, item: StackItem) -> Result<(), ScriptHelperError> {
    builder
        .emit_push_stack_item(item)
        .map(|_| ())
        .map_err(map_vm_error)
}

fn map_vm_error(error: VmError) -> ScriptHelperError {
    ScriptHelperError::ScriptBuild(error.to_string())
}
