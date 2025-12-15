use neo_core::network::p2p::payloads::{signer::Signer, transaction::Transaction};
use neo_core::persistence::DataCache;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::witness::Witness;
use neo_core::{IVerifiable, UInt160, WitnessScope};
use std::sync::Arc;

fn sample_hash(tag: u8) -> UInt160 {
    UInt160::from_bytes(&[tag; 20]).unwrap()
}

fn make_engine_with_signer(signer: Signer) -> ApplicationEngine {
    const TEST_GAS_LIMIT: i64 = 100_000_000;
    let snapshot = Arc::new(DataCache::new(false));

    let mut tx = Transaction::new();
    tx.set_signers(vec![signer]);
    tx.add_witness(Witness::new());
    let container: Arc<dyn IVerifiable> = Arc::new(tx);

    ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        Default::default(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine")
}

#[test]
fn check_witness_global_scope_allows_signer() {
    let account = sample_hash(0x01);
    let signer = Signer::new(account, WitnessScope::GLOBAL);
    let engine = make_engine_with_signer(signer);

    assert!(engine.check_witness_hash(&account).unwrap());
    assert!(!engine.check_witness_hash(&sample_hash(0x02)).unwrap());
}

#[test]
fn check_witness_custom_contracts_match_current_script_hash() {
    let account = sample_hash(0x10);
    let allowed_contract = sample_hash(0x11);
    let other_contract = sample_hash(0x12);

    let mut signer = Signer::new(account, WitnessScope::CUSTOM_CONTRACTS);
    signer.allowed_contracts.push(allowed_contract);

    let mut engine = make_engine_with_signer(signer);

    engine.set_current_script_hash(Some(allowed_contract));
    assert!(engine.check_witness_hash(&account).unwrap());

    engine.set_current_script_hash(Some(other_contract));
    assert!(!engine.check_witness_hash(&account).unwrap());
}

#[test]
fn check_witness_called_by_entry_matches_depth() {
    let account = sample_hash(0x20);
    let signer = Signer::new(account, WitnessScope::CALLED_BY_ENTRY);
    let mut engine = make_engine_with_signer(signer);

    // Entry context (no calling context).
    let entry_hash = sample_hash(0x21);
    engine
        .load_script(vec![0x40], CallFlags::ALL, Some(entry_hash))
        .unwrap();
    assert!(engine.check_witness_hash(&account).unwrap());

    let entry_ctx = engine.invocation_stack()[0].clone();

    // Direct call from entry.
    let direct_hash = sample_hash(0x22);
    engine
        .load_script(vec![0x40], CallFlags::ALL, Some(direct_hash))
        .unwrap();
    let direct_ctx = engine.invocation_stack()[1].clone();
    {
        let state_arc = engine.current_execution_state().unwrap();
        let mut state = state_arc.lock();
        state.calling_context = Some(entry_ctx.clone());
    }
    assert!(engine.check_witness_hash(&account).unwrap());

    // Nested call (depth > 1) should fail CalledByEntry.
    let nested_hash = sample_hash(0x23);
    engine
        .load_script(vec![0x40], CallFlags::ALL, Some(nested_hash))
        .unwrap();
    {
        let state_arc = engine.current_execution_state().unwrap();
        let mut state = state_arc.lock();
        state.calling_context = Some(direct_ctx.clone());
    }
    assert!(!engine.check_witness_hash(&account).unwrap());
}
