use super::*;

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_payloads::{Block, Header, Transaction, Witness};
use neo_runtime::BlockOrigin;
use neo_storage::DataCache;

use crate::pipeline::block_validation::MIN_TIMESTAMP_MS;
use crate::pipeline::stage_traits::StageContext;

fn pipeline() -> VerifiedImportPipeline {
    VerifiedImportPipeline::new(
        Arc::new(ProtocolSettings::default()),
        Arc::new(DataCache::new(false)),
        None,
    )
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
    assert!(!normal.bulk_sync);

    let bulk = StageContext::for_verified_import(7, true);
    assert_eq!(bulk.origin, BlockOrigin::TrustedLocal);
    assert_eq!(bulk.current_height, 7);
    assert!(bulk.bulk_sync);
}

#[tokio::test]
async fn verified_import_pipeline_rejects_merkle_before_parent_lookup() {
    let mut tx = Transaction::new();
    tx.set_nonce(1);
    tx.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    let block = Block::from_parts(base_header(), vec![tx]);

    let err = pipeline()
        .verify(&stage_context(), &block)
        .await
        .expect_err("stale merkle root must fail validation first");
    let message = err.to_string();

    assert!(message.contains("Merkle root mismatch"));
    assert!(
        !message.contains("previous block not found"),
        "consensus-witness stage should not run after validation fails: {message}"
    );
}

#[tokio::test]
async fn verified_import_pipeline_runs_consensus_witness_after_validation() {
    let mut header = base_header();
    header.witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    let block = Block::from_parts(header, Vec::new());

    let err = pipeline()
        .verify(&stage_context(), &block)
        .await
        .expect_err("valid structure should reach parent consensus lookup");

    assert!(err.to_string().contains("previous block not found"));
}
