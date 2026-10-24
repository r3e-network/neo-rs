//! `ApplicationEngine` hands the VM a raw pointer to itself
//! (`ExecutionEngine::set_interop_host`), which means the pointer goes stale as
//! soon as the engine moves. Every VM entry point re-attaches it; these tests
//! pin that down, because a missing refresh would dereference freed stack memory
//! instead of failing loudly.

use neo_core::ScriptBuilder;
use neo_core::UInt160;
use neo_core::Verifiable;
use neo_core::WitnessScope;
use neo_core::ledger::{Block, BlockHeader};
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::CallFlags;
use neo_core::smart_contract::TriggerType;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::witness::Witness;
use neo_vm::{OpCode, VmState};
use std::sync::Arc;

fn engine() -> ApplicationEngine {
    let mut tx = Transaction::new();
    tx.set_nonce(0);
    tx.set_attributes(Vec::new());
    tx.set_signers(vec![Signer::new(
        UInt160::zero(),
        WitnessScope::CALLED_BY_ENTRY,
    )]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container: Arc<dyn Verifiable> = Arc::new(tx);
    let block = Block::new(
        BlockHeader {
            index: 1,
            ..Default::default()
        },
        Vec::new(),
    );
    ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        Arc::new(DataCache::new(false)),
        Some(block),
        ProtocolSettings::mainnet(),
        10_00000000,
        None,
    )
    .expect("engine")
}

#[test]
fn moved_engine_still_dispatches_host_syscalls() {
    // `ApplicationEngine::new` calls `attach_host`, so the stored pointer refers
    // to the local that the constructor returns. Boxing moves the engine after
    // that; the next entry point has to re-attach before dispatching.
    let mut engine = Box::new(engine());

    // GetNotifications is not handled by the fast interpreter, so this script
    // forces the fork-aware path and therefore a real host callback.
    let mut script = ScriptBuilder::new();
    script.emit_opcode(OpCode::PUSHNULL);
    script
        .emit_syscall("System.Runtime.GetNotifications")
        .expect("emit syscall");
    engine
        .load_script(script.to_array(), CallFlags::ALL, None)
        .expect("load script");

    assert_eq!(
        engine.execute_allow_fault(),
        VmState::HALT,
        "host dispatch failed after the engine moved"
    );
    let notifications = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_array()
        .expect("GetNotifications returns an array");
    assert!(
        notifications.is_empty(),
        "a transaction without notifications must yield an empty array"
    );
}
