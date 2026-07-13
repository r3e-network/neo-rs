use super::*;

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_payloads::{Block, Header, Transaction, Witness};
use neo_runtime::BlockOrigin;
use neo_storage::DataCache;

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
