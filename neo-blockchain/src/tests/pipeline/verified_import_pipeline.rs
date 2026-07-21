use super::*;

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_crypto::Secp256r1Crypto;
use neo_execution::{PreverifiedSignatureCache, preverify_standard_witness_signatures};
use neo_payloads::{Block, Header, Transaction, Witness};
use neo_runtime::BlockOrigin;
use neo_storage::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::script_builder::redeem_script::RedeemScript;

use crate::pipeline::block_validation::MIN_TIMESTAMP_MS;
use crate::pipeline::stage_traits::StageContext;

fn native_provider() -> Arc<neo_native_contracts::StandardNativeProvider> {
    Arc::new(neo_native_contracts::StandardNativeProvider::new())
}

fn pipeline() -> VerifiedImportPipeline<
    neo_native_contracts::StandardNativeProvider,
    neo_storage::EmptyCacheBacking,
> {
    VerifiedImportPipeline::new(
        Arc::new(ProtocolSettings::default()),
        Arc::new(DataCache::new(false)),
        native_provider(),
    )
}

#[test]
fn verified_import_pipeline_accepts_concrete_native_provider() {
    let pipeline: VerifiedImportPipeline<
        neo_native_contracts::StandardNativeProvider,
        neo_storage::EmptyCacheBacking,
    > = VerifiedImportPipeline::new(
        Arc::new(ProtocolSettings::default()),
        Arc::new(DataCache::new(false)),
        Arc::new(neo_native_contracts::StandardNativeProvider::new()),
    );

    let context: SnapshotConsensusWitnessContext<
        neo_native_contracts::StandardNativeProvider,
        neo_storage::EmptyCacheBacking,
    > = SnapshotConsensusWitnessContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::new(DataCache::new(false)),
        Arc::new(neo_native_contracts::StandardNativeProvider::new()),
    );

    let _ = (pipeline, context);
}

#[test]
fn verified_import_pipeline_requires_explicit_native_provider() {
    let source = include_str!("../../pipeline/verified_import_pipeline.rs");
    let context = include_str!("../../pipeline/consensus_witness_stage/context.rs");

    assert!(source.contains("pub struct VerifiedImportPipeline<P, B>"));
    assert!(
        source.contains(
            "consensus_witness: NeoConsensusWitnessStage<SnapshotConsensusWitnessContext<P, B>>"
        ),
        "verified import pipeline should pass the concrete provider type into the consensus-witness context"
    );
    assert!(context.contains("pub struct SnapshotConsensusWitnessContext<P, B>"));
    assert!(
        context.contains("native_contract_provider: Arc<P>"),
        "snapshot consensus-witness context should store Arc<P>, not erase the provider internally"
    );
    assert!(
        context.contains("type NativeProvider: NativeContractProvider"),
        "consensus-witness context trait should expose the captured provider type"
    );
    assert!(
        !context.contains("fn native_contract_provider_for_vm"),
        "consensus-witness context should pass the concrete provider to generic witness helpers"
    );
}

fn stage_context() -> StageContext {
    StageContext::for_verified_import(0, false)
}

fn base_header() -> Header {
    let mut header = Header::new();
    header.set_index(1);
    header.set_timestamp(MIN_TIMESTAMP_MS + 15_000);
    header
}

fn cached_aborting_block(snapshot: &DataCache) -> (Block, Arc<PreverifiedSignatureCache>) {
    let private_key = [37u8; 32];
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive public key");
    let standard_script = RedeemScript::signature_redeem_script(&public_key);
    let mut aborting_script = standard_script.clone();
    aborting_script.push(neo_vm::OpCode::ABORT.byte());

    let mut parent_header = Header::new();
    parent_header.set_index(0);
    parent_header.set_timestamp(MIN_TIMESTAMP_MS);
    parent_header.set_next_consensus(
        Witness::new_with_scripts(Vec::new(), aborting_script.clone()).script_hash(),
    );
    let parent = Block::from_parts(parent_header, Vec::new());
    let parent_hash = parent.hash();
    crate::ledger_records::LedgerRecords::write_on_persist_records(snapshot, &parent, &parent_hash)
        .expect("seed parent ledger records");

    let mut child_header = base_header();
    child_header.set_prev_hash(parent_hash);
    let settings = ProtocolSettings::default();
    let sign_data =
        neo_payloads::get_sign_data_vec(&child_header, settings.network).expect("child sign data");
    let signature = Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign child header");
    let mut invocation = ScriptBuilder::new();
    invocation.emit_push(&signature);
    let invocation = invocation.to_array();
    let standard_witness = Witness::new_with_scripts(invocation.clone(), standard_script);
    let signature_cache = preverify_standard_witness_signatures(&sign_data, &standard_witness)
        .expect("canonical standard witness cache");
    child_header.witness = Witness::new_with_scripts(invocation, aborting_script);

    (Block::from_parts(child_header, Vec::new()), signature_cache)
}

#[test]
fn verified_import_stage_context_marks_bulk_as_trusted_local() {
    let normal = StageContext::for_verified_import(7, false);
    assert_eq!(normal.origin, BlockOrigin::Rpc);
    assert_eq!(normal.current_height, 7);
    assert!(!normal.trusted_replay);

    let bulk = StageContext::for_verified_import(7, true);
    assert_eq!(bulk.origin, BlockOrigin::TrustedLocal);
    assert_eq!(bulk.current_height, 7);
    assert!(bulk.trusted_replay);
}

#[test]
fn verified_import_pipeline_rejects_merkle_before_parent_lookup() {
    let mut tx = Transaction::new();
    tx.set_nonce(1);
    tx.set_script(vec![neo_vm::OpCode::PUSH1.byte()]);
    let block = Block::from_parts(base_header(), vec![tx]);

    let err = pipeline()
        .verify(&stage_context(), &block)
        .expect_err("stale merkle root must fail validation first");
    let message = err.to_string();

    assert!(message.contains("Merkle root mismatch"));
    assert!(
        !message.contains("previous block not found"),
        "consensus-witness stage should not run after validation fails: {message}"
    );
}

#[test]
fn verified_import_pipeline_runs_consensus_witness_after_validation() {
    let mut header = base_header();
    header.witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
    let block = Block::from_parts(header, Vec::new());

    let err = pipeline()
        .verify(&stage_context(), &block)
        .expect_err("valid structure should reach parent consensus lookup");

    assert!(err.to_string().contains("previous block not found"));
}

#[test]
fn verified_import_cache_path_still_runs_the_canonical_witness_vm_fence() {
    let snapshot = Arc::new(DataCache::new(false));
    let (block, signature_cache) = cached_aborting_block(snapshot.as_ref());
    let pipeline = VerifiedImportPipeline::new(
        Arc::new(ProtocolSettings::default()),
        snapshot,
        native_provider(),
    );

    let error = pipeline
        .verify_with_signature_cache(&stage_context(), &block, Some(signature_cache))
        .expect_err("a cached CheckSig result must not authorize an ABORTing witness script");
    assert!(
        error
            .to_string()
            .contains("consensus witness verification failed")
    );
}

#[test]
fn verified_import_pipeline_exposes_no_non_witness_validation_entry_point() {
    let source = include_str!("../../pipeline/verified_import_pipeline.rs");

    assert!(!source.contains("fn validate_block("));
    assert!(!source.contains("fn validate(&self"));
    assert!(source.contains("pub fn verify_with_signature_cache("));
    assert!(source.contains("pub fn verify_block_with_signature_cache("));
    assert!(source.contains("self.validate.validate(ctx, block)?"));
    assert!(source.contains(".verify_block_with_signature_cache(block, signature_cache)"));
}
