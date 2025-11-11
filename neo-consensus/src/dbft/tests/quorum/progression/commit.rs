use super::super::super::helpers::*;
use super::super::super::*;
use hex_literal::hex;

#[test]
fn quorum_progression() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    let primary_index = set.index_of(primary).unwrap();
    let proposal = Hash256::new(hex!(
        "b74f66f80de93df5b8f2671db9add7907f3229e6a49a5bb5bbd93a91d832d49a"
    ));

    let request = build_signed(
        &priv_keys[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareRequest {
            proposal_hash: proposal,
            height: HEIGHT,
            tx_hashes: vec![],
        },
    );
    matches!(
        engine.process_message(request).unwrap(),
        QuorumDecision::Pending
    );

    let mut responded = Vec::new();
    let primary_response = build_signed(
        &priv_keys[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareResponse {
            proposal_hash: proposal,
        },
    );
    engine.process_message(primary_response).unwrap();
    responded.push(primary_index);

    let responders = (0..set.len())
        .filter(|idx| *idx != primary_index)
        .take(set.quorum() - 1)
        .collect::<Vec<_>>();
    for idx in responders.iter().copied() {
        let response = build_signed(
            &priv_keys[idx as usize],
            idx as u16,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(response).unwrap();
        responded.push(idx as usize);
    }

    let mut expected_missing = Vec::new();
    for (idx, validator) in set.iter().enumerate() {
        if !responded.contains(&idx) {
            expected_missing.push(validator.id);
        }
    }
    let missing = engine.missing_validators(MessageKind::PrepareResponse);
    assert_eq!(missing, expected_missing);

    let mut decision = QuorumDecision::Pending;
    for idx in responded.iter().copied() {
        let commit = build_signed(
            &priv_keys[idx as usize],
            idx as u16,
            ViewNumber::ZERO,
            ConsensusMessage::Commit {
                proposal_hash: proposal,
            },
        );
        decision = engine.process_message(commit).unwrap();
    }
    assert!(matches!(
        decision,
        QuorumDecision::Proposal {
            kind: MessageKind::Commit,
            proposal: observed,
            missing
        } if observed == proposal && missing.is_empty()
    ));

    let participation = engine.participation();
    assert_eq!(
        participation
            .get(&MessageKind::PrepareRequest)
            .cloned()
            .unwrap(),
        vec![primary]
    );
    let expected_responses = responded
        .iter()
        .map(|idx| ValidatorId(*idx as u16))
        .collect::<Vec<_>>();
    assert_eq!(
        participation
            .get(&MessageKind::PrepareResponse)
            .cloned()
            .unwrap(),
        expected_responses
    );
    assert_eq!(
        participation
            .get(&MessageKind::Commit)
            .map(|entries| entries.len())
            .unwrap(),
        responded.len()
    );
}
