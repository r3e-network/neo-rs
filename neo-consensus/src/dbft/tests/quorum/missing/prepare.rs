use super::super::super::{helpers::*, HEIGHT};
use crate::{
    message::{ConsensusMessage, MessageKind},
    state::ConsensusState,
    DbftEngine, ViewNumber,
};
use neo_base::hash::Hash256;

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
