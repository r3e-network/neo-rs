use super::*;

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_payloads::{Block, Header, Transaction, Witness};
use neo_primitives::UInt256;
use neo_runtime::BlockOrigin;

use crate::block_validation::MIN_TIMESTAMP_MS;
use crate::pipeline::stage_traits::{PipelineStage, StageContext, StageId};

#[derive(Debug)]
struct MockValidateContext {
    settings: Arc<ProtocolSettings>,
    prev_hash: Option<UInt256>,
    prev_timestamp: Option<u64>,
    validators_count: i32,
}

impl MockValidateContext {
    fn new() -> Self {
        let settings = Arc::new(ProtocolSettings::default());
        Self {
            validators_count: settings.validators_count,
            settings,
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
        self.validators_count = settings.validators_count;
        self.settings = Arc::new(settings);
        self
    }
}

impl ValidateContext for MockValidateContext {
    fn settings(&self) -> Arc<ProtocolSettings> {
        self.settings.clone()
    }

    fn prev_block_hash(&self, _height: u32) -> Option<UInt256> {
        self.prev_hash
    }

    fn prev_block_timestamp(&self, _height: u32) -> Option<u64> {
        self.prev_timestamp
    }

    fn validators_count(&self) -> i32 {
        self.validators_count
    }
}

fn stage(ctx: MockValidateContext) -> NeoValidateStage {
    NeoValidateStage::new(Arc::new(ctx))
}

fn stage_context(current_height: u32) -> StageContext {
    StageContext {
        origin: BlockOrigin::Sync,
        current_height,
        bulk_sync: false,
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

#[tokio::test]
async fn validate_stage_accepts_valid_empty_child_block() {
    let prev_hash = UInt256::from([1u8; 32]);
    let block = empty_block(1, prev_hash, MIN_TIMESTAMP_MS + 15_000);
    let stage = stage(MockValidateContext::new().with_prev(prev_hash, MIN_TIMESTAMP_MS));
    let output = stage
        .execute(&stage_context(0), &block)
        .await
        .expect("valid block");

    assert!(output.performed_work);
}

#[tokio::test]
async fn validate_stage_rejects_height_mismatch() {
    let block = empty_block(3, UInt256::default(), MIN_TIMESTAMP_MS + 15_000);
    let stage = stage(MockValidateContext::new());
    let err = stage
        .execute(&stage_context(0), &block)
        .await
        .expect_err("height mismatch must fail");

    assert!(err.to_string().contains("height mismatch"));
}

#[tokio::test]
async fn validate_stage_rejects_previous_hash_mismatch() {
    let expected_prev = UInt256::from([1u8; 32]);
    let actual_prev = UInt256::from([2u8; 32]);
    let block = empty_block(1, actual_prev, MIN_TIMESTAMP_MS + 15_000);
    let stage = stage(MockValidateContext::new().with_prev(expected_prev, MIN_TIMESTAMP_MS));
    let err = stage
        .execute(&stage_context(0), &block)
        .await
        .expect_err("previous hash mismatch must fail");

    assert!(err.to_string().contains("previous hash mismatch"));
}

#[tokio::test]
async fn validate_stage_uses_protocol_transaction_limit() {
    let mut settings = ProtocolSettings::default();
    settings.max_transactions_per_block = 1;
    let block = block_with_transactions(1, 2);
    let stage = stage(MockValidateContext::new().with_settings(settings));
    let err = stage
        .execute(&stage_context(0), &block)
        .await
        .expect_err("protocol tx limit must fail");

    assert!(
        err.to_string()
            .contains("Transaction count 2 exceeds maximum 1")
    );
}

#[tokio::test]
async fn validate_stage_rejects_invalid_header_witness() {
    let mut block = empty_block(1, UInt256::default(), MIN_TIMESTAMP_MS + 15_000);
    block.header.witness = Witness::new_with_scripts(vec![], vec![0xFF]);
    let stage = stage(MockValidateContext::new());
    let err = stage
        .execute(&stage_context(0), &block)
        .await
        .expect_err("invalid header witness must fail");

    assert!(err.to_string().contains("Invalid witness script"));
}
