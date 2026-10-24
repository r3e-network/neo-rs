//! Regression tests for values that cross the boundary between the fast
//! interpreter (`neo_vm::StackValue`) and the canonical `StackItem` model.
//!
//! `StackValue::Pointer` carries only a script offset, while `StackItem::Pointer`
//! also needs the owning script. C# lets a `PUSHA` halt with a `Pointer` on the
//! result stack, so the bridge has to rebuild the pointer rather than reject it.
//! `Interop` and `Iterator` have no `StackItem` counterpart at all and must fault
//! the invocation -- never panic -- because the script decides what lands on the
//! result stack.

use neo_core::UInt160;
use neo_core::Verifiable;
use neo_core::WitnessScope;
use neo_core::hardfork::Hardfork;
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

/// The fast interpreter only runs once Gorgon semantics are active (MainNet
/// activates it at height 12,020,000), so pin the activation height to 0 rather
/// than waiting for the real fork height.
fn gorgon_settings() -> ProtocolSettings {
    let mut settings = ProtocolSettings::mainnet();
    settings.hardforks.insert(Hardfork::HfGorgon, 0);
    settings
}

fn gorgon_engine() -> ApplicationEngine {
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
        gorgon_settings(),
        10_00000000,
        None,
    )
    .expect("engine")
}

#[test]
fn pusha_pointer_on_result_stack_halts_with_pointer() {
    // PUSHA 0 pushes Pointer(0) and the script then falls off its end, so the
    // evaluation stack handed back to the host is [Pointer(0)].
    let mut engine = gorgon_engine();
    engine
        .load_script(vec![OpCode::PUSHA.byte(), 0, 0, 0, 0], CallFlags::ALL, None)
        .expect("load script");

    let state = engine.execute_allow_fault();

    assert_eq!(
        state,
        VmState::HALT,
        "C# halts here; faulting would diverge from the reference implementation"
    );
    let stack = engine.result_stack();
    assert_eq!(stack.len(), 1, "expected exactly the pushed pointer");
    let pointer = stack
        .peek(0)
        .expect("result item")
        .get_pointer()
        .expect("result item should be a pointer");
    assert_eq!(pointer.position(), 0);
}
