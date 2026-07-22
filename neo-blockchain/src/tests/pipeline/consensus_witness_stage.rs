use super::*;

use std::fmt;
use std::sync::Arc;

use neo_config::{ChainSpecProvider, NeoChainSpec, ProtocolSettings};
use neo_crypto::Secp256r1Crypto;
use neo_error::{CoreError, CoreResult};
use neo_execution::{PreverifiedSignatureCache, preverify_standard_witness_signatures};
use neo_payloads::{Block, Header, Witness};
use neo_primitives::UInt256;
use neo_runtime::BlockOrigin;
use neo_storage::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::script_builder::redeem_script::RedeemScript;

use crate::pipeline::stage_traits::{ConsensusWitnessStage, PipelineStage, StageContext, StageId};

struct MockConsensusWitnessContext {
    chain_spec: Arc<NeoChainSpec>,
    snapshot: DataCache,
    parent: Option<ParentHeaderContext>,
}

impl fmt::Debug for MockConsensusWitnessContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockConsensusWitnessContext")
            .field(
                "validators_count",
                &self.chain_spec.protocol_settings().validators_count,
            )
            .field("parent", &self.parent)
            .finish_non_exhaustive()
    }
}

impl MockConsensusWitnessContext {
    fn new(parent: Option<ParentHeaderContext>) -> Self {
        Self {
            chain_spec: neo_test_fixtures::test_chain_spec(ProtocolSettings::default()),
            snapshot: DataCache::new(false),
            parent,
        }
    }
}

impl ChainSpecProvider for MockConsensusWitnessContext {
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        Arc::clone(&self.chain_spec)
    }
}

impl ConsensusWitnessContext for MockConsensusWitnessContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

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

fn signed_standard_witness(header: &Header) -> (Witness, Arc<PreverifiedSignatureCache>) {
    let private_key = [31u8; 32];
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive public key");
    let sign_data = neo_payloads::get_sign_data_vec(header, ProtocolSettings::default().network)
        .expect("header sign data");
    let signature = Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign header");
    let mut invocation = ScriptBuilder::new();
    invocation.emit_push(&signature);
    let witness = Witness::new_with_scripts(
        invocation.to_array(),
        RedeemScript::signature_redeem_script(&public_key),
    );
    let cache = preverify_standard_witness_signatures(&sign_data, &witness)
        .expect("canonical standard witness cache");
    (witness, cache)
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
fn consensus_witness_stage_accepts_exact_cache_without_skipping_neovm() {
    let parent_hash = UInt256::from([2u8; 32]);
    let mut block = block(1, parent_hash, 20, Witness::empty());
    let (witness, cache) = signed_standard_witness(&block.header);
    let parent = ParentHeaderContext {
        hash: parent_hash,
        index: 0,
        timestamp: 10,
        next_consensus: witness.script_hash(),
    };
    block.header.witness = witness;

    stage(Some(parent))
        .verify_block_with_signature_cache(&block, Some(cache))
        .expect("exact preverification should retain canonical witness success");
}

#[test]
fn consensus_witness_cache_hit_still_executes_the_remaining_verification_script() {
    let parent_hash = UInt256::from([3u8; 32]);
    let mut block = block(1, parent_hash, 20, Witness::empty());
    let (standard_witness, cache) = signed_standard_witness(&block.header);

    let mut aborting_script = standard_witness.verification_script.clone();
    aborting_script.push(neo_vm::OpCode::ABORT.byte());
    let aborting_witness =
        Witness::new_with_scripts(standard_witness.invocation_script, aborting_script);
    let parent = ParentHeaderContext {
        hash: parent_hash,
        index: 0,
        timestamp: 10,
        next_consensus: aborting_witness.script_hash(),
    };
    block.header.witness = aborting_witness;

    let error = stage(Some(parent))
        .verify_block_with_signature_cache(&block, Some(cache))
        .expect_err("a cached CheckSig result must not skip the following ABORT");
    assert!(
        error
            .to_string()
            .contains("consensus witness verification failed")
    );
}

#[test]
fn consensus_witness_cache_miss_falls_back_to_ordinary_crypto() {
    let parent_hash = UInt256::from([4u8; 32]);
    let mut block = block(1, parent_hash, 20, Witness::empty());
    let (witness, _) = signed_standard_witness(&block.header);
    let unrelated_sign_data = [0xA5; 36];
    let unrelated_cache = preverify_standard_witness_signatures(&unrelated_sign_data, &witness)
        .expect("canonical witness under another exact message");
    let parent = ParentHeaderContext {
        hash: parent_hash,
        index: 0,
        timestamp: 10,
        next_consensus: witness.script_hash(),
    };
    block.header.witness = witness;

    stage(Some(parent))
        .verify_block_with_signature_cache(&block, Some(unrelated_cache))
        .expect("message mismatch must fall back to ordinary signature verification");
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
fn consensus_witness_stage_rejects_primary_index_outside_validator_set() {
    let witness = true_witness();
    let parent_hash = UInt256::from([1u8; 32]);
    let parent = ParentHeaderContext {
        hash: parent_hash,
        index: 0,
        timestamp: 10,
        next_consensus: witness.script_hash(),
    };
    let mut block = block(1, parent_hash, 20, witness);
    block.header.set_primary_index(
        u8::try_from(ProtocolSettings::default().validators_count)
            .expect("default validator count fits in u8"),
    );

    let error = stage(Some(parent))
        .verify_consensus_witness(&stage_context(), &block)
        .expect_err("primary index equal to validator count must fail");

    assert!(
        error
            .to_string()
            .to_ascii_lowercase()
            .contains("primary index"),
        "unexpected error: {error}"
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

#[test]
fn cached_valid_signature_cannot_bypass_parent_next_consensus() {
    let parent_hash = UInt256::from([5u8; 32]);
    let mut block = block(1, parent_hash, 20, Witness::empty());
    let (witness, cache) = signed_standard_witness(&block.header);
    block.header.witness = witness;
    let parent = ParentHeaderContext {
        hash: parent_hash,
        index: 0,
        timestamp: 10,
        next_consensus: Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()])
            .script_hash(),
    };

    let error = stage(Some(parent))
        .verify_block_with_signature_cache(&block, Some(cache))
        .expect_err("a cached valid signature cannot replace the parent account check");
    assert!(
        error
            .to_string()
            .contains("consensus witness verification failed")
    );
}
