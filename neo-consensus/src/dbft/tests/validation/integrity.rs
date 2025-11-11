use super::super::helpers::*;
use super::super::*;
use neo_crypto::ecdsa::SIGNATURE_SIZE;

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
    msg.signature = SignatureBytes([0u8; SIGNATURE_SIZE]);
    let err = engine.process_message(msg).unwrap_err();
    assert!(matches!(
        err,
        ConsensusError::InvalidSignature(validator) if validator == primary
    ));
}
