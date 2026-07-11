//! # neo-native-contracts::tests::neo_token::governance_writer_tests
//!
//! Test module grouping governance writer tests behavior coverage for neo-
//! native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `candidate_registration`: NEO candidate registration coverage.
//! - `candidates`: NEO candidate storage codecs.
//! - `payments`: governance payment coverage.
//! - `transfers`: wallet transfer RPC handlers.
//! - `voting`: NEO voting coverage.

use super::*;
use neo_config::ProtocolSettings;
use neo_execution::native_contract::build_native_contract_state;
use neo_execution::{ApplicationEngine, Contract};
use neo_payloads::VerifiableContainer;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_primitives::{CallFlags, TriggerType, WitnessScope};
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::VmState;
use std::sync::Arc;

use crate::test_support::deploy_native;

mod candidate_registration;
mod candidates;
mod payments;
mod transfers;
mod voting;

fn candidate_pubkey() -> ECPoint {
    // A valid secp256r1 public key (a Neo N3 standby validator).
    ECPoint::from_bytes(
        &hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap(),
    )
    .unwrap()
}

/// Runs `method(pubkey)` on NeoToken via System.Contract.Call, signed (Global)
/// by `signer`, against the shared `snapshot`. Returns the final VM state.
fn call(snapshot: Arc<DataCache>, signer: UInt160, pubkey: &[u8], method: &str) -> VmState {
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container = Arc::new(VerifiableContainer::from(tx));

    let mut builder = ScriptBuilder::new();
    builder.emit_push(pubkey);
    builder.emit_push_int(1);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(method.as_bytes());
    builder.emit_push(&NeoToken::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");

    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        ProtocolSettings::default(),
        2000_00000000, // > the 1000-GAS register price
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
    )
    .expect("engine builds");
    engine
        .load_script(builder.to_array(), CallFlags::ALL, None)
        .expect("script loads");
    engine.execute_allow_fault()
}

fn seeded_snapshot() -> Arc<DataCache> {
    let cache = DataCache::new(false);
    let neo_state = build_native_contract_state(&NeoToken, &ProtocolSettings::default(), 0);
    deploy_native(&cache, &neo_state);
    seed_register_price(&cache);
    Arc::new(cache)
}

fn seed_register_price(cache: &DataCache) {
    cache.add(
        NeoToken::register_price_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            DEFAULT_REGISTER_PRICE,
        ))),
    );
}
