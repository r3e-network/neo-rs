use super::*;
use std::sync::Arc;

use crate::test_support::{committee_address, deploy_native, sample_committee, seed_committee};
use neo_execution::native_contract::build_native_contract_state;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_payloads::{Block, Header, VerifiableContainer};
use neo_primitives::{CallFlags, TriggerType, UInt160, WitnessScope};
use neo_storage::persistence::DataCache;
use neo_vm::VmState;
use neo_vm::script_builder::ScriptBuilder;

/// Runs `Treasury::verify()` via System.Contract.Call, signed (Global) by
/// `signer`. Returns the final VM state and the boolean result.
fn call_verify(
    snapshot: Arc<DataCache>,
    signer: UInt160,
    settings: ProtocolSettings,
    block_height: u32,
) -> (VmState, Option<bool>) {
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container = Arc::new(VerifiableContainer::from(tx));

    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(0);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push("verify".as_bytes());
    builder.emit_push(&Treasury::script_hash().to_array());
    builder.emit_syscall("System.Contract.Call").expect("call");

    let mut header = Header::new();
    header.set_index(block_height);
    let block = Block::from_parts(header, Vec::new());

    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        snapshot,
        Some(block),
        settings,
        10_000_000,
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
    )
    .expect("engine builds");
    engine
        .load_script(builder.to_array(), CallFlags::ALL, None)
        .expect("script loads");
    let state = engine.execute_allow_fault();
    let top = engine
        .result_stack()
        .peek(0)
        .ok()
        .and_then(|item| item.as_bool().ok());
    (state, top)
}

#[test]
fn verify_is_true_only_with_the_committee_witness() {
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    // Deploy Treasury directly so this test can focus on verify witness
    // behavior; activation boundaries are covered separately.
    let settings = ProtocolSettings::default();
    let faun_height = settings
        .hardforks
        .activation_height(Hardfork::HfFaun)
        .expect("default settings schedule Faun");
    deploy_native(
        &cache,
        &build_native_contract_state(&Treasury, &settings, faun_height),
    );
    let snapshot = Arc::new(cache);

    // Signed by the committee multisig address -> true.
    let (state, result) = call_verify(
        Arc::clone(&snapshot),
        committee_address(&committee),
        settings.clone(),
        faun_height,
    );
    assert_eq!(state, VmState::HALT, "verify must HALT");
    assert_eq!(result, Some(true), "the committee witness verifies");

    // Signed by an unrelated account -> false (a clean HALT, no fault).
    let stranger = UInt160::from_bytes(&[0x21; 20]).unwrap();
    let (state, result) = call_verify(
        Arc::clone(&snapshot),
        stranger,
        settings.clone(),
        faun_height,
    );
    assert_eq!(state, VmState::HALT, "verify must HALT");
    assert_eq!(result, Some(false), "a non-committee witness fails");
}
