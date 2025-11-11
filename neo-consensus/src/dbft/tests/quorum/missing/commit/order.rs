use super::super::super::super::{helpers::*, HEIGHT};
use crate::{
    message::{ConsensusMessage, MessageKind},
    state::ConsensusState,
    validator::ValidatorId,
    DbftEngine, QuorumDecision, ViewNumber,
};
use hex_literal::hex;
use neo_base::hash::Hash256;

#[test]
fn commit_quorum_reports_missing_validators_sorted() {
    let (set, priv_keys) = generate_validators_with_count(7);
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    let primary_index = set.index_of(primary).unwrap();
    let proposal = Hash256::new(hex!(
        "6f9f17959d0baf7e40b3f12694d21fb3acbd0a5f1c11472edb7458060f0b43a1"
    ));

    let prepare_request = build_signed(
        &priv_keys[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareRequest {
            proposal_hash: proposal,
            height: HEIGHT,
            tx_hashes: vec![],
        },
    );
    engine.process_message(prepare_request).unwrap();

    let response_order = [6u16, 0, 5, 3, 2, 4, 1];
    for validator in response_order {
        let response = build_signed(
            &priv_keys[validator as usize],
            validator,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(response).unwrap();
    }

    let expected_missing: Vec<_> = (0..set.len() as u16).map(ValidatorId).collect();
    assert_eq!(
        engine.missing_validators(MessageKind::Commit),
        expected_missing
    );

    let mut decision = QuorumDecision::Pending;
    let committers = [6u16, 0, 5, 3, 2];
    for validator in committers {
        let commit = build_signed(
            &priv_keys[validator as usize],
            validator,
            ViewNumber::ZERO,
            ConsensusMessage::Commit {
                proposal_hash: proposal,
            },
        );
        decision = engine.process_message(commit).unwrap();
    }

    match decision {
        QuorumDecision::Proposal {
            kind,
            missing,
            proposal: observed,
        } => {
            assert_eq!(kind, MessageKind::Commit);
            assert_eq!(observed, proposal);
            assert_eq!(missing, vec![ValidatorId(1), ValidatorId(4)]);
        }
        other => panic!("expected commit proposal, got {other:?}"),
    }
}
