use super::*;

use std::fmt;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_payloads::{Block, Header, Witness};
use neo_primitives::UInt256;
use neo_runtime::BlockOrigin;
use neo_storage::DataCache;

use crate::pipeline::stage_traits::{ConsensusWitnessStage, PipelineStage, StageContext, StageId};

struct MockConsensusWitnessContext {
    settings: Arc<ProtocolSettings>,
    snapshot: DataCache,
    parent: Option<ParentHeaderContext>,
}

impl fmt::Debug for MockConsensusWitnessContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockConsensusWitnessContext")
            .field("validators_count", &self.settings.validators_count)
            .field("parent", &self.parent)
            .finish_non_exhaustive()
    }
}

impl MockConsensusWitnessContext {
    fn new(parent: Option<ParentHeaderContext>) -> Self {
        Self {
            settings: Arc::new(ProtocolSettings::default()),
            snapshot: DataCache::new(false),
            parent,
        }
    }
}

impl ConsensusWitnessContext for MockConsensusWitnessContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn snapshot(&self) -> &DataCache {
        &self.snapshot
    }

    fn native_contract_provider(&self) -> Arc<neo_native_contracts::StandardNativeProvider> {
        Arc::new(neo_native_contracts::StandardNativeProvider::new())
    }

    fn parent_header(&self, _block: &Block) -> CoreResult<ParentHeaderContext> {
        self.parent
            .ok_or_else(|| CoreError::other("previous block not found"))
    }
}

fn stage(
    parent: Option<ParentHeaderContext>,
) -> NeoConsensusWitnessStage<MockConsensusWitnessContext> {
    NeoConsensusWitnessStage::new(Arc::new(MockConsensusWitnessContext::new(parent)))
}

fn stage_context() -> StageContext {
    StageContext {
        origin: BlockOrigin::Sync,
        current_height: 0,
        trusted_replay: false,
    }
}

fn true_witness() -> Witness {
    Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()])
}

fn block(index: u32, parent_hash: UInt256, timestamp: u64, witness: Witness) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(parent_hash);
    header.set_timestamp(timestamp);
    header.witness = witness;
    Block::from_parts(header, Vec::new())
}

#[test]
fn consensus_witness_stage_id_is_consensus_witness() {
    let stage = stage(None);
    assert_eq!(PipelineStage::id(&stage), StageId::ConsensusWitness);
}

#[test]
fn consensus_witness_stage_owns_concrete_context_type() {
    let source = include_str!("../../pipeline/consensus_witness_stage.rs");

    assert!(
        source.contains("pub struct NeoConsensusWitnessStage<C>"),
        "consensus-witness stage should preserve the concrete context type"
    );
    assert!(
        !source.contains("C = SnapshotConsensusWitnessContext"),
        "consensus-witness stage must not default to an erased provider context"
    );
    assert!(
        source.contains("ctx: Arc<C>"),
        "consensus-witness stage should own Arc<C>, not Arc<dyn ConsensusWitnessContext>"
    );
    assert!(
        !source.contains("ctx: Arc<dyn ConsensusWitnessContext>"),
        "owned homogeneous stage context should not be erased to dyn"
    );
}

#[test]
fn consensus_witness_stage_accepts_authorized_header_witness() {
    let witness = true_witness();
    let parent_hash = UInt256::from([1u8; 32]);
    let parent = ParentHeaderContext {
        hash: parent_hash,
        index: 0,
        timestamp: 10,
        next_consensus: witness.script_hash(),
    };
    let block = block(1, parent_hash, 20, witness);

    stage(Some(parent))
        .verify_consensus_witness(&stage_context(), &block)
        .expect("matching parent next-consensus should authorize the witness");
}

#[test]
fn consensus_witness_stage_rejects_missing_parent() {
    let block = block(1, UInt256::from([1u8; 32]), 20, true_witness());

    let err = stage(None)
        .verify_consensus_witness(&stage_context(), &block)
        .expect_err("missing parent must fail");

    assert!(err.to_string().contains("previous block not found"));
}

#[test]
fn consensus_witness_stage_rejects_non_increasing_timestamp() {
    let witness = true_witness();
    let parent_hash = UInt256::from([1u8; 32]);
    let parent = ParentHeaderContext {
        hash: parent_hash,
        index: 0,
        timestamp: 20,
        next_consensus: witness.script_hash(),
    };
    let block = block(1, parent_hash, 20, witness);

    let err = stage(Some(parent))
        .verify_consensus_witness(&stage_context(), &block)
        .expect_err("non-increasing timestamp must fail");

    assert!(
        err.to_string()
            .contains("timestamp not after previous block")
    );
}

#[test]
fn consensus_witness_stage_rejects_wrong_consensus_account() {
    let witness = true_witness();
    let parent_hash = UInt256::from([1u8; 32]);
    let parent = ParentHeaderContext {
        hash: parent_hash,
        index: 0,
        timestamp: 10,
        next_consensus: Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH2.byte()])
            .script_hash(),
    };
    let block = block(1, parent_hash, 20, witness);

    let err = stage(Some(parent))
        .verify_consensus_witness(&stage_context(), &block)
        .expect_err("wrong parent next-consensus must fail");

    assert!(
        err.to_string()
            .contains("consensus witness verification failed")
    );
}
