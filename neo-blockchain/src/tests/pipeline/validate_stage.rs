use super::*;

use std::sync::Arc;

use neo_config::{ChainSpecProvider, NeoChainSpec, ProtocolSettings};
use neo_payloads::{Block, Header, Transaction, Witness};
use neo_primitives::UInt256;
use neo_runtime::BlockOrigin;

use crate::block_validation::MIN_TIMESTAMP_MS;
use crate::pipeline::stage_traits::{PipelineStage, StageContext, StageId};

#[derive(Debug)]
struct MockValidateContext {
    chain_spec: Arc<NeoChainSpec>,
    prev_hash: Option<UInt256>,
    prev_timestamp: Option<u64>,
}

impl MockValidateContext {
    fn new() -> Self {
        let settings = ProtocolSettings::default();
        Self {
            chain_spec: neo_test_fixtures::test_chain_spec(settings),
            prev_hash: None,
            prev_timestamp: None,
        }
    }

    fn with_prev(mut self, prev_hash: UInt256, prev_timestamp: u64) -> Self {
        self.prev_hash = Some(prev_hash);
        self.prev_timestamp = Some(prev_timestamp);
        self
    }

    fn with_settings(mut self, settings: ProtocolSettings) -> Self {
        self.chain_spec = neo_test_fixtures::test_chain_spec(settings);
        self
    }
}

impl ChainSpecProvider for MockValidateContext {
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        Arc::clone(&self.chain_spec)
    }
}

impl ValidateContext for MockValidateContext {
    fn prev_block_hash(&self, _height: u32) -> Option<UInt256> {
        self.prev_hash
    }

    fn prev_block_timestamp(&self, _height: u32) -> Option<u64> {
        self.prev_timestamp
    }
}

fn stage(ctx: MockValidateContext) -> NeoValidateStage<MockValidateContext> {
    NeoValidateStage::new(Arc::new(ctx))
}

fn stage_context(current_height: u32) -> StageContext {
    StageContext {
        origin: BlockOrigin::Sync,
        current_height,
        trusted_replay: false,
    }
}

fn empty_block(index: u32, prev_hash: UInt256, timestamp: u64) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(prev_hash);
    header.set_timestamp(timestamp);
    header.set_primary_index(0);
    Block::from_parts(header, Vec::new())
}

fn block_with_transactions(index: u32, tx_count: usize) -> Block {
    let mut block = empty_block(index, UInt256::default(), MIN_TIMESTAMP_MS + 15_000);
    block.transactions = (0..tx_count)
        .map(|i| {
            let mut tx = Transaction::new();
            tx.set_nonce(i as u32 + 1);
            tx.set_script(vec![i as u8 + 1]);
            tx
        })
        .collect();
    block
        .try_rebuild_merkle_root()
        .expect("valid tx merkle root");
    block
}

#[test]
fn validate_stage_id_is_validate() {
    let stage = stage(MockValidateContext::new());
    assert_eq!(PipelineStage::id(&stage), StageId::Validate);
}

#[test]
fn validate_stage_owns_concrete_context_type() {
    let source = include_str!("../../pipeline/validate_stage.rs");
    let context = include_str!("../../pipeline/validate_stage/context.rs");

    assert!(
        source.contains(
            "pub struct NeoValidateStage<C = SnapshotValidateContext<neo_storage::EmptyCacheBacking>>"
        ),
        "validate stage should preserve the concrete context type"
    );
    assert!(
        source.contains("ctx: Arc<C>"),
        "validate stage should own Arc<C>, not Arc<dyn ValidateContext>"
    );
    assert!(
        !source.contains("ctx: Arc<dyn ValidateContext>"),
        "owned homogeneous validate context should not be erased to dyn"
    );
    assert!(context.contains("ChainSpecProvider<ChainSpec = NeoChainSpec>"));
    assert!(
        !context.contains("fn settings(&self)"),
        "validation contexts must expose the canonical chain spec instead of a second settings root"
    );
    assert!(
        !context.contains("fn validators_count(&self)"),
        "validator count must be derived from the canonical chain spec"
    );
}

#[test]
fn validate_stage_accepts_valid_empty_child_block() {
    let prev_hash = UInt256::from([1u8; 32]);
    let block = empty_block(1, prev_hash, MIN_TIMESTAMP_MS + 15_000);
    let stage = stage(MockValidateContext::new().with_prev(prev_hash, MIN_TIMESTAMP_MS));
    let output = stage
        .execute(&stage_context(0), &block)
        .expect("valid block");

    assert!(output.performed_work);
}

#[test]
fn validate_stage_rejects_height_mismatch() {
    let block = empty_block(3, UInt256::default(), MIN_TIMESTAMP_MS + 15_000);
    let stage = stage(MockValidateContext::new());
    let err = stage
        .execute(&stage_context(0), &block)
        .expect_err("height mismatch must fail");

    assert!(err.to_string().contains("height mismatch"));
}

#[test]
fn validate_stage_rejects_previous_hash_mismatch() {
    let expected_prev = UInt256::from([1u8; 32]);
    let actual_prev = UInt256::from([2u8; 32]);
    let block = empty_block(1, actual_prev, MIN_TIMESTAMP_MS + 15_000);
    let stage = stage(MockValidateContext::new().with_prev(expected_prev, MIN_TIMESTAMP_MS));
    let err = stage
        .execute(&stage_context(0), &block)
        .expect_err("previous hash mismatch must fail");

    assert!(err.to_string().contains("previous hash mismatch"));
}

#[test]
fn validate_stage_reads_validator_count_from_chain_spec() {
    let mut settings = ProtocolSettings::default();
    settings.validators_count = 1;
    let mut block = empty_block(1, UInt256::default(), MIN_TIMESTAMP_MS + 15_000);
    block.header.set_primary_index(1);
    let stage = stage(MockValidateContext::new().with_settings(settings));

    let error = stage
        .execute(&stage_context(0), &block)
        .expect_err("primary index outside the chain spec validator set must fail");

    assert!(
        error
            .to_string()
            .contains("Primary index 1 exceeds maximum validator count 1")
    );
}

#[test]
fn validate_stage_does_not_enforce_production_transaction_limit() {
    let mut settings = ProtocolSettings::default();
    settings.max_transactions_per_block = 1;
    let block = block_with_transactions(1, 2);
    let stage = stage(MockValidateContext::new().with_settings(settings));
    stage
        .execute(&stage_context(0), &block)
        .expect("production transaction limit must not reject a verified block");
}

#[test]
fn validate_stage_does_not_use_local_wall_clock_policy() {
    let previous = MIN_TIMESTAMP_MS;
    let block = empty_block(1, UInt256::default(), u64::MAX);
    let stage = stage(MockValidateContext::new().with_prev(UInt256::default(), previous));

    stage
        .execute(&stage_context(0), &block)
        .expect("future-dated signed headers are not rejected by local wall clock");
}

#[test]
fn validate_stage_rejects_invalid_header_witness() {
    let mut block = empty_block(1, UInt256::default(), MIN_TIMESTAMP_MS + 15_000);
    block.header.witness = Witness::new_with_scripts(vec![], vec![0xFF]);
    let stage = stage(MockValidateContext::new());
    let err = stage
        .execute(&stage_context(0), &block)
        .expect_err("invalid header witness must fail");

    assert!(err.to_string().contains("Invalid witness script"));
}
