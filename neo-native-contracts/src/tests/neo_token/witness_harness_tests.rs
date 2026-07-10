use neo_config::ProtocolSettings;
use neo_execution::ApplicationEngine;
use neo_payloads::VerifiableContainer;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_primitives::{CallFlags, TriggerType, UInt160, WitnessScope};
use neo_storage::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::VmState;
use std::sync::Arc;

/// Builds a script that calls `System.Runtime.CheckWitness(hash)`.
fn check_witness_script(hash: &UInt160) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&hash.to_array());
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("CheckWitness syscall");
    builder.to_array()
}

/// Runs `script` through a fresh Application-trigger engine whose container
/// is a transaction signed (Global scope) by each hash in `signers`.
/// Returns the final VM state and the boolean on top of the result stack.
fn run_signed(script: Vec<u8>, signers: &[UInt160]) -> (VmState, bool) {
    let mut tx = Transaction::new();
    tx.set_signers(
        signers
            .iter()
            .map(|h| Signer::new(*h, WitnessScope::GLOBAL))
            .collect(),
    );
    tx.set_witnesses(signers.iter().map(|_| Witness::empty()).collect());
    let container = Arc::new(VerifiableContainer::from(tx));

    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        10_000_000,
        neo_execution::NoDiagnostic,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
    )
    .expect("engine builds");
    engine
        .load_script(script, CallFlags::READ_ONLY, None)
        .expect("script loads");
    let state = engine.execute_allow_fault();
    let top = engine
        .result_stack()
        .peek(0)
        .ok()
        .and_then(|item| item.as_bool().ok())
        .unwrap_or(false);
    (state, top)
}

#[test]
fn checkwitness_true_for_signer_false_for_others() {
    let signer = UInt160::from_bytes(&[0x11; 20]).unwrap();
    let stranger = UInt160::from_bytes(&[0x22; 20]).unwrap();

    // The signed hash → CheckWitness true.
    let (state, ok) = run_signed(check_witness_script(&signer), &[signer]);
    assert_eq!(state, VmState::HALT, "script must HALT");
    assert!(ok, "CheckWitness must be true for a Global-scope signer");

    // A different hash → CheckWitness false (still a clean HALT).
    let (state2, ok2) = run_signed(check_witness_script(&stranger), &[signer]);
    assert_eq!(state2, VmState::HALT, "script must HALT");
    assert!(!ok2, "CheckWitness must be false for a non-signer");
}
