use alloc::vec::Vec;

use neo_crypto::Secp256r1Verify;

use crate::message::MessageKind;
use crate::validator::ValidatorId;
use crate::{
    error::ConsensusError,
    message::SignedMessage,
    state::{ConsensusState, QuorumDecision, SnapshotState},
    validator::ValidatorSet,
};
use alloc::collections::BTreeMap;

pub struct DbftEngine {
    state: ConsensusState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayResult {
    Applied(QuorumDecision),
    Skipped,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ConsensusMessage, MessageKind, ViewNumber};
    use crate::state::{ConsensusState, SnapshotState};
    use crate::validator::{Validator, ValidatorId, ValidatorSet};
    use crate::ConsensusError;
    use hex_literal::hex;
    use neo_base::{encoding::SliceReader, hash::Hash256, NeoDecode, NeoEncode};
    use neo_crypto::{
        ecc256::PrivateKey, ecdsa::SIGNATURE_SIZE, Keypair, Secp256r1Sign, SignatureBytes,
    };
    use alloc::format;
    use rand::{rngs::StdRng, RngCore, SeedableRng};

    const HEIGHT: u64 = 10;

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

        // Prepare request from validator 0
        let msg0 = build_signed(
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
            engine.process_message(msg0).unwrap(),
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

        // Responses from 1 and 2 push quorum over threshold (4 validators -> quorum 3)
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
                proposal: p,
                missing
            } if p == proposal && missing.is_empty()
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

    #[test]
    fn rejects_invalid_signature() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let proposal = Hash256::new([0x11; 32]);

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();

        let mut msg = build_signed(
            &priv_keys[primary_index],
            primary.0,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareRequest {
                proposal_hash: proposal,
                height: HEIGHT,
                tx_hashes: vec![],
            },
        );
        // Corrupt signature
        msg.signature = SignatureBytes([0u8; SIGNATURE_SIZE]);
        let err = engine.process_message(msg).unwrap_err();
        assert!(matches!(
            err,
            ConsensusError::InvalidSignature(validator) if validator == primary
        ));
    }

    #[test]
    fn change_view_quorum_advances_state() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);

        let target_view = ViewNumber(1);
        let participants: Vec<usize> = (0..set.len()).collect();
        for idx in participants.iter().copied().take(set.quorum() - 1) {
            let change = build_signed(
                &priv_keys[idx],
                idx as u16,
                ViewNumber::ZERO,
                ConsensusMessage::ChangeView {
                    new_view: target_view,
                    reason: crate::message::ChangeViewReason::Timeout,
                },
            );
            matches!(
                engine.process_message(change).unwrap(),
                QuorumDecision::Pending
            );
        }

        let decision = engine
            .process_message(build_signed(
                &priv_keys[participants[set.quorum() - 1]],
                participants[set.quorum() - 1] as u16,
                ViewNumber::ZERO,
                ConsensusMessage::ChangeView {
                    new_view: target_view,
                    reason: crate::message::ChangeViewReason::Timeout,
                },
            ))
            .unwrap();
        assert!(matches!(
            decision,
            QuorumDecision::ViewChange { new_view, .. } if new_view == target_view
        ));
        assert_eq!(engine.state().view(), target_view);
        assert!(engine.participation().is_empty());

        let new_primary = set.primary_id(HEIGHT, target_view).unwrap();
        let new_primary_index = set.index_of(new_primary).unwrap();
        let request = build_signed(
            &priv_keys[new_primary_index],
            new_primary.0,
            target_view,
            ConsensusMessage::PrepareRequest {
                proposal_hash: Hash256::new([0xAA; 32]),
                height: HEIGHT,
                tx_hashes: vec![],
            },
        );
        matches!(
            engine.process_message(request).unwrap(),
            QuorumDecision::Pending
        );
    }

    #[test]
    fn missing_for_prepare_request_targets_primary() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();

        let missing_before = engine.missing_validators(MessageKind::PrepareRequest);
        assert_eq!(missing_before, vec![primary]);

        let proposal = Hash256::new([0x12; 32]);
        let primary_index = set.index_of(primary).unwrap();
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
        engine.process_message(request).unwrap();

        let missing_after = engine.missing_validators(MessageKind::PrepareRequest);
        assert!(missing_after.is_empty());
    }

    #[test]
    fn commit_missing_is_deferred_until_stage_active() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let proposal = Hash256::new([0x42; 32]);

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();
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
        engine.process_message(request).unwrap();

        let primary_response = build_signed(
            &priv_keys[primary_index],
            primary.0,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(primary_response).unwrap();

        assert_eq!(
            engine.missing_validators(MessageKind::Commit),
            vec![primary]
        );

        let commit = build_signed(
            &priv_keys[primary_index],
            primary.0,
            ViewNumber::ZERO,
            ConsensusMessage::Commit {
                proposal_hash: proposal,
            },
        );
        engine.process_message(commit).unwrap();

        assert!(engine.missing_validators(MessageKind::Commit).is_empty());
        assert!(engine
            .expected_participants(MessageKind::Commit)
            .is_none());
    }

    #[test]
    fn prepare_request_expectation_tracks_primary() {
        let (set, _privs) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let engine = DbftEngine::new(state);
        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        assert_eq!(
            engine.expected_participants(MessageKind::PrepareRequest),
            Some(vec![primary])
        );
    }

    #[test]
    fn prepare_request_expectation_updates_with_view_and_height() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let initial_primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        assert_eq!(
            engine.expected_participants(MessageKind::PrepareRequest),
            Some(vec![initial_primary])
        );

        let target_view = ViewNumber(1);
        for idx in 0..set.quorum() {
            let change = build_signed(
                &priv_keys[idx],
                idx as u16,
                ViewNumber::ZERO,
                ConsensusMessage::ChangeView {
                    new_view: target_view,
                    reason: crate::message::ChangeViewReason::Timeout,
                },
            );
            let decision = engine.process_message(change).unwrap();
            if idx + 1 == set.quorum() {
                assert!(matches!(
                    decision,
                    QuorumDecision::ViewChange { new_view, .. } if new_view == target_view
                ));
            }
        }

        let new_primary = set.primary_id(HEIGHT, target_view).unwrap();
        assert_eq!(
            engine.expected_participants(MessageKind::PrepareRequest),
            Some(vec![new_primary])
        );

        let next_height = HEIGHT + 1;
        engine.advance_height(next_height).unwrap();
        let height_primary = set
            .primary_id(next_height, ViewNumber::ZERO)
            .unwrap();
        assert_eq!(
            engine.expected_participants(MessageKind::PrepareRequest),
            Some(vec![height_primary])
        );
    }

    #[test]
    fn expected_participants_for_commit_follow_prepare_responses() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);

        assert!(engine.expected_participants(MessageKind::Commit).is_none());

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();
        let proposal = Hash256::new([0x31; 32]);
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
        engine.process_message(request).unwrap();

        let responders = [primary_index, (primary_index + 1) % set.len()];
        for idx in responders.iter().copied() {
            let response = build_signed(
                &priv_keys[idx],
                idx as u16,
                ViewNumber::ZERO,
                ConsensusMessage::PrepareResponse {
                    proposal_hash: proposal,
                },
            );
            engine.process_message(response).unwrap();
        }

        let expected = responders
            .iter()
            .map(|idx| ValidatorId(*idx as u16))
            .collect::<Vec<_>>();
        assert_eq!(
            engine.expected_participants(MessageKind::Commit).unwrap(),
            expected
        );

        for idx in responders.iter().copied() {
            let commit = build_signed(
                &priv_keys[idx],
                idx as u16,
                ViewNumber::ZERO,
                ConsensusMessage::Commit {
                    proposal_hash: proposal,
                },
            );
            engine.process_message(commit).unwrap();
        }

        assert!(engine
            .expected_participants(MessageKind::Commit)
            .is_none());
    }

    #[test]
    fn expected_participants_for_change_view_activate_with_messages() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        assert!(engine
            .expected_participants(MessageKind::ChangeView)
            .is_none());

        let change = build_signed(
            &priv_keys[0],
            0,
            ViewNumber::ZERO,
            ConsensusMessage::ChangeView {
                new_view: ViewNumber(1),
                reason: crate::message::ChangeViewReason::Timeout,
            },
        );
        engine.process_message(change).unwrap();

        let expected = set.iter().map(|validator| validator.id).collect::<Vec<_>>();
        assert_eq!(
            engine
                .expected_participants(MessageKind::ChangeView)
                .unwrap(),
            expected
        );
    }

    #[test]
    fn prepare_response_requires_registered_proposal() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set);
        let mut engine = DbftEngine::new(state);

        let response = build_signed(
            &priv_keys[1],
            1,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: Hash256::new([0x55; 32]),
            },
        );
        let err = engine.process_message(response).unwrap_err();
        assert!(matches!(err, ConsensusError::MissingProposal));
    }

    #[test]
    fn commit_requires_registered_proposal() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);

        let commit = build_signed(
            &priv_keys[0],
            0,
            ViewNumber::ZERO,
            ConsensusMessage::Commit {
                proposal_hash: Hash256::new([0x44; 32]),
            },
        );
        let err = engine.process_message(commit).unwrap_err();
        assert!(matches!(err, ConsensusError::MissingProposal));
    }

    #[test]
    fn commit_requires_prepare_response_from_validator() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let proposal = Hash256::new([0x99; 32]);

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();
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
        engine.process_message(request).unwrap();

        let other_index = (0..set.len()).find(|idx| *idx != primary_index).unwrap();
        let commit = build_signed(
            &priv_keys[other_index],
            other_index as u16,
            ViewNumber::ZERO,
            ConsensusMessage::Commit {
                proposal_hash: proposal,
            },
        );
        let err = engine.process_message(commit).unwrap_err();
        assert!(matches!(
            err,
            ConsensusError::MissingPrepareResponse { validator }
                if validator == ValidatorId(other_index as u16)
        ));

        let response = build_signed(
            &priv_keys[other_index],
            other_index as u16,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(response).unwrap();

        let commit = build_signed(
            &priv_keys[other_index],
            other_index as u16,
            ViewNumber::ZERO,
            ConsensusMessage::Commit {
                proposal_hash: proposal,
            },
        );
        matches!(
            engine.process_message(commit).unwrap(),
            QuorumDecision::Pending
        );
    }

    #[test]
    fn prepare_request_must_come_from_primary() {
        let (set, priv_keys) = generate_validators();
        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();
        let non_primary_index = (primary_index + 1) % set.len();
        let non_primary_id = ValidatorId(non_primary_index as u16);

        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let proposal = Hash256::new([0x21; 32]);

        let bad = build_signed(
            &priv_keys[non_primary_index],
            non_primary_id.0,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareRequest {
                proposal_hash: proposal,
                height: HEIGHT,
                tx_hashes: vec![],
            },
        );
        let err = engine.process_message(bad).unwrap_err();
        assert!(matches!(
            err,
            ConsensusError::InvalidPrimary {
                expected,
                actual
            } if expected == primary && actual == non_primary_id
        ));

        let good = build_signed(
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
            engine.process_message(good).unwrap(),
            QuorumDecision::Pending
        );
    }

    #[test]
    fn duplicate_prepare_response_rejected() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let proposal = Hash256::new([0x22; 32]);

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();
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
        engine.process_message(request).unwrap();

        let other_index = (0..set.len()).find(|idx| *idx != primary_index).unwrap();
        let response = build_signed(
            &priv_keys[other_index],
            other_index as u16,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(response.clone()).unwrap();
        let err = engine.process_message(response).unwrap_err();
        assert!(matches!(
            err,
            ConsensusError::DuplicateMessage {
                kind: MessageKind::PrepareResponse,
                validator
            } if validator == ValidatorId(other_index as u16)
        ));
    }

    #[test]
    fn duplicate_commit_rejected() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let proposal = Hash256::new([0x77; 32]);

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();
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
        engine.process_message(request).unwrap();

        let response = build_signed(
            &priv_keys[primary_index],
            primary.0,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(response).unwrap();

        let commit = build_signed(
            &priv_keys[primary_index],
            primary.0,
            ViewNumber::ZERO,
            ConsensusMessage::Commit {
                proposal_hash: proposal,
            },
        );
        engine.process_message(commit.clone()).unwrap();
        let err = engine.process_message(commit).unwrap_err();
        assert!(matches!(
            err,
            ConsensusError::DuplicateMessage {
                kind: MessageKind::Commit,
                validator
            } if validator == primary
        ));
    }

    #[test]
    fn change_view_rejects_previous_view_messages() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let target_view = ViewNumber(1);

        for idx in 0..set.quorum() {
            let change = build_signed(
                &priv_keys[idx],
                idx as u16,
                ViewNumber::ZERO,
                ConsensusMessage::ChangeView {
                    new_view: target_view,
                    reason: crate::message::ChangeViewReason::Timeout,
                },
            );
            engine.process_message(change).unwrap();
        }
        assert_eq!(engine.state().view(), target_view);

        let stale = build_signed(
            &priv_keys[set.quorum()],
            set.quorum() as u16,
            ViewNumber::ZERO,
            ConsensusMessage::ChangeView {
                new_view: ViewNumber(2),
                reason: crate::message::ChangeViewReason::Timeout,
            },
        );
        let err = engine.process_message(stale).unwrap_err();
        assert!(matches!(
            err,
            ConsensusError::StaleMessage {
                kind: MessageKind::ChangeView,
                ..
            }
        ));
    }

    #[test]
    fn snapshot_roundtrip_preserves_participation() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let proposal = Hash256::new([0x33; 32]);

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();
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
        engine.process_message(request).unwrap();

        let other_index = (0..set.len()).find(|idx| *idx != primary_index).unwrap();
        let response = build_signed(
            &priv_keys[other_index],
            other_index as u16,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(response.clone()).unwrap();

        let snapshot = engine.snapshot();
        let mut bytes = Vec::new();
        snapshot.neo_encode(&mut bytes);
        let mut reader = SliceReader::new(bytes.as_slice());
        let decoded = SnapshotState::neo_decode(&mut reader).unwrap();
        assert_eq!(snapshot, decoded);
        let mut restored = DbftEngine::from_snapshot(set, decoded).unwrap();
        let err = restored.process_message(response).unwrap_err();
        assert!(matches!(
            err,
            ConsensusError::DuplicateMessage {
                kind: MessageKind::PrepareResponse,
                validator
            } if validator == ValidatorId(other_index as u16)
        ));
        assert_eq!(
            engine.expected_participants(MessageKind::Commit),
            restored.expected_participants(MessageKind::Commit)
        );
    }

    #[test]
    fn snapshot_restores_prepare_request_expectation() {
        let (set, _privs) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let engine = DbftEngine::new(state);
        let snapshot = engine.snapshot();
        let restored = DbftEngine::from_snapshot(set.clone(), snapshot).unwrap();

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        assert_eq!(
            restored.expected_participants(MessageKind::PrepareRequest),
            Some(vec![primary])
        );
        assert_eq!(
            restored.missing_validators(MessageKind::PrepareRequest),
            vec![primary]
        );
    }

    #[test]
    fn advance_height_resets_state() {
        let (set, priv_keys) = generate_validators();
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        let mut engine = DbftEngine::new(state);
        let proposal = Hash256::new([0x44; 32]);

        let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = set.index_of(primary).unwrap();
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
        engine.process_message(request).unwrap();

        let err = engine.advance_height(HEIGHT).unwrap_err();
        assert!(matches!(
            err,
            ConsensusError::InvalidHeightTransition { .. }
        ));

        engine.advance_height(HEIGHT + 1).unwrap();
        assert_eq!(engine.state().height(), HEIGHT + 1);
        assert_eq!(engine.state().view(), ViewNumber::ZERO);
        assert!(engine.state().proposal().is_none());
        assert_eq!(engine.state().tally(MessageKind::PrepareResponse), 0);
    }

    fn generate_validators() -> (ValidatorSet, Vec<PrivateKey>) {
        let mut privs = Vec::new();
        let mut validators = Vec::new();
        let mut rng = StdRng::seed_from_u64(1337);
        for idx in 0..4u16 {
            let mut bytes = [0u8; 32];
            rng.fill_bytes(&mut bytes);
            let private = PrivateKey::new(bytes);
            let keypair = Keypair::from_private(private.clone()).unwrap();
            validators.push(Validator {
                id: ValidatorId(idx),
                public_key: keypair.public_key,
                alias: Some(format!("validator-{idx}")),
            });
            privs.push(private);
        }
        (ValidatorSet::new(validators), privs)
    }

    fn build_signed(
        private: &PrivateKey,
        index: u16,
        view: ViewNumber,
        message: ConsensusMessage,
    ) -> SignedMessage {
        let mut signed = SignedMessage::new(
            HEIGHT,
            view,
            ValidatorId(index),
            message,
            SignatureBytes([0u8; SIGNATURE_SIZE]),
        );
        let digest = signed.digest();
        let raw_sig = private
            .secp256r1_sign(digest.as_ref())
            .expect("signing succeeds");
        signed.signature = raw_sig;
        signed
    }
}

impl DbftEngine {
    pub fn new(state: ConsensusState) -> Self {
        Self { state }
    }

    pub fn from_snapshot(
        validators: ValidatorSet,
        snapshot: SnapshotState,
    ) -> Result<Self, ConsensusError> {
        ConsensusState::from_snapshot(validators, snapshot).map(Self::new)
    }

    pub fn state(&self) -> &ConsensusState {
        &self.state
    }

    pub fn snapshot(&self) -> SnapshotState {
        self.state.snapshot()
    }

    pub fn into_state(self) -> ConsensusState {
        self.state
    }

    pub fn participation(&self) -> BTreeMap<MessageKind, Vec<ValidatorId>> {
        self.state.participation_by_kind()
    }

    pub fn tallies(&self) -> BTreeMap<MessageKind, usize> {
        self.state.tallies()
    }

    pub fn quorum_threshold(&self) -> usize {
        self.state.quorum_threshold()
    }

    pub fn primary(&self) -> Option<ValidatorId> {
        self.state.primary()
    }

    pub fn missing_validators(&self, kind: MessageKind) -> Vec<ValidatorId> {
        self.state.missing_validators(kind)
    }

    pub fn expected_participants(&self, kind: MessageKind) -> Option<Vec<ValidatorId>> {
        self.state.expected_participants(kind)
    }

    pub fn process_message(
        &mut self,
        message: SignedMessage,
    ) -> Result<QuorumDecision, ConsensusError> {
        let kind = message.kind();
        let pending_view = match &message.message {
            crate::message::ConsensusMessage::ChangeView { new_view, .. } => Some(*new_view),
            _ => None,
        };
        self.verify_signature(&message)?;
        self.state.register_message(message)?;
        match self.state.quorum(kind) {
            QuorumDecision::ViewChange { new_view, missing } => {
                if let Some(target) = pending_view {
                    if target == new_view {
                        self.state.apply_view_change(new_view);
                    }
                } else {
                    self.state.apply_view_change(new_view);
                }
                Ok(QuorumDecision::ViewChange { new_view, missing })
            }
            decision => Ok(decision),
        }
    }

    pub fn replay_messages<I>(&mut self, messages: I) -> Vec<ReplayResult>
    where
        I: IntoIterator<Item = SignedMessage>,
    {
        messages
            .into_iter()
            .map(|m| match self.process_message(m) {
                Ok(decision) => ReplayResult::Applied(decision),
                Err(_) => ReplayResult::Skipped,
            })
            .collect()
    }

    pub fn advance_height(&mut self, new_height: u64) -> Result<(), ConsensusError> {
        self.state.advance_height(new_height)
    }

    fn verify_signature(&self, message: &SignedMessage) -> Result<(), ConsensusError> {
        let validator = self
            .state
            .validators()
            .get(message.validator)
            .ok_or(ConsensusError::UnknownValidator(message.validator))?;
        let digest = message.digest();
        validator
            .public_key
            .secp256r1_verify(digest.as_ref(), &message.signature)
            .map_err(|_| ConsensusError::InvalidSignature(message.validator))
    }
}
