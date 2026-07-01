//! # neo-native-contracts::tests::policy_contract::policy_writer_tests
//!
//! Test module grouping policy writer tests behavior coverage for neo-native-
//! contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `block_account`: blocked-account policy coverage.
//! - `recover_fund`: fund recovery policy coverage.
//! - `whitelist`: policy whitelist coverage.

use super::*;
use crate::test_support::{committee_address, deploy_native, sample_committee, seed_committee};
use neo_config::ProtocolSettings;
use neo_execution::contract_state::ContractState;
use neo_execution::native_contract::build_native_contract_state;
use neo_manifest::{ContractManifest, ContractMethodDescriptor, NefFile};
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_payloads::{Block, BlockHeader};
use neo_primitives::{TriggerType, Verifiable, WitnessScope};
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::VmState;
use std::sync::Arc;

/// ProtocolSettings with HF_Faun scheduled from genesis.
fn faun_settings() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfFaun, 0);
    settings
}

/// Runs `method(args...)` on PolicyContract via System.Contract.Call,
/// signed (Global) by `signer`, against the shared `snapshot`. The closure
/// must push the call arguments in REVERSE order (deepest first). Returns
/// the final VM state and the finished engine (for result-stack and
/// notification assertions).
fn call_policy_engine<F>(
    snapshot: Arc<DataCache>,
    signer: UInt160,
    settings: ProtocolSettings,
    block: Option<Block>,
    method: &str,
    argc: i64,
    push_args_reversed: F,
) -> (VmState, ApplicationEngine)
where
    F: FnOnce(&mut ScriptBuilder),
{
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container: Arc<dyn Verifiable> = Arc::new(tx);

    let mut builder = ScriptBuilder::new();
    push_args_reversed(&mut builder);
    builder.emit_push_int(argc);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(method.as_bytes());
    builder.emit_push(&PolicyContract::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        block,
        settings,
        2000_00000000,
        None,
    )
    .expect("engine builds");
    engine
        .load_script(builder.to_array(), CallFlags::ALL, None)
        .expect("script loads");
    let state = engine.execute_allow_fault();
    (state, engine)
}

/// [`call_policy_engine`] reduced to the final VM state and the boolean on
/// top of the result stack (if any).
fn call_policy<F>(
    snapshot: Arc<DataCache>,
    signer: UInt160,
    settings: ProtocolSettings,
    block: Option<Block>,
    method: &str,
    argc: i64,
    push_args_reversed: F,
) -> (VmState, Option<bool>)
where
    F: FnOnce(&mut ScriptBuilder),
{
    let (state, engine) = call_policy_engine(
        snapshot,
        signer,
        settings,
        block,
        method,
        argc,
        push_args_reversed,
    );
    let top = engine
        .result_stack()
        .peek(0)
        .ok()
        .and_then(|item| item.as_bool().ok());
    (state, top)
}

fn returning_user_contract(hash: UInt160) -> ContractState {
    let nef = NefFile::new(
        "policy-blocked-call-test".to_string(),
        vec![
            neo_vm_rs::OpCode::PUSH1.byte(),
            neo_vm_rs::OpCode::RET.byte(),
        ],
    );
    let mut manifest = ContractManifest::new("BlockedCallFixture".to_string());
    manifest.abi.methods.push(
        ContractMethodDescriptor::new(
            "answer".to_string(),
            Vec::new(),
            ContractParameterType::Integer,
            0,
            true,
        )
        .expect("method descriptor"),
    );
    ContractState::new(7, hash, nef, manifest)
}

mod block_account;
mod recover_fund;
mod whitelist;
