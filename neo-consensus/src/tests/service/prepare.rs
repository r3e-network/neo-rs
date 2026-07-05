use super::helpers::{create_validators_with_keys, sign_commit, sign_payload};
use crate::messages::{
    CommitMessage, ConsensusPayload, PrepareRequestMessage, PrepareResponseMessage,
};
use crate::{ConsensusError, ConsensusMessageType};
use crate::{ConsensusEvent, ConsensusService};
use neo_crypto::MerkleTree;
use neo_primitives::UInt256;
use tokio::sync::mpsc;

use super::super::helpers::{ConsensusBlockFields, current_timestamp};

#[tokio::test]
async fn consensus_merkle_root_matches_core_merkle_tree() {
    assert_eq!(
        ConsensusBlockFields::compute_merkle_root(&[]),
        UInt256::zero()
    );

    for hashes in [
        vec![UInt256::from([0x11; 32])],
        vec![UInt256::from([0x11; 32]), UInt256::from([0x22; 32])],
        vec![
            UInt256::from([0x11; 32]),
            UInt256::from([0x22; 32]),
            UInt256::from([0x33; 32]),
        ],
    ] {
        assert_eq!(
            ConsensusBlockFields::compute_merkle_root(&hashes),
            MerkleTree::compute_root(&hashes).expect("non-empty merkle root")
        );
    }
}

fn proposed_block_hash(
    service: &ConsensusService,
    tx_hashes: &[UInt256],
    timestamp: u64,
    nonce: u64,
) -> UInt256 {
    let merkle_root = ConsensusBlockFields::compute_merkle_root(tx_hashes);
    ConsensusBlockFields::compute_header_hash(
        service.context().version,
        service.context().prev_hash,
        merkle_root,
        timestamp,
        nonce,
        service.context().block_index,
        service.context().primary_index(),
        service.context().next_consensus,
    )
}

#[path = "prepare/backup_request.rs"]
mod backup_request;
#[path = "prepare/commit_flow.rs"]
mod commit_flow;
#[path = "prepare/initial_round.rs"]
mod initial_round;
#[path = "prepare/primary_proposal.rs"]
mod primary_proposal;
#[path = "prepare/response_and_cache.rs"]
mod response_and_cache;
