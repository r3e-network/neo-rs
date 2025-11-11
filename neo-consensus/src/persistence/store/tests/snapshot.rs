use alloc::vec;

use neo_crypto::Secp256r1Sign;
use neo_store::MemoryStore;

use crate::{
    message::{ConsensusMessage, MessageKind, SignedMessage, ViewNumber},
    persistence::store::{clear_snapshot, load_engine, persist_engine},
    validator::ValidatorId,
    SnapshotKey,
};

use super::fixtures::TestContext;

#[test]
fn persist_and_restore_snapshot() {
    let store = MemoryStore::new();
    let key = SnapshotKey {
        network: TestContext::NETWORK,
    };

    let mut ctx = TestContext::build();
    let primary = ctx
        .validators
        .primary_id(TestContext::HEIGHT, ViewNumber::ZERO)
        .unwrap();
    let primary_index = ctx.validators.index_of(primary).unwrap();
    let proposal = TestContext::proposal_hash();

    let request = build_signed(
        TestContext::HEIGHT,
        &ctx.priv_keys[primary_index],
        primary,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareRequest {
            proposal_hash: proposal,
            height: TestContext::HEIGHT,
            tx_hashes: vec![],
        },
    );
    ctx.engine.process_message(request).unwrap();

    let responder_index = (0..ctx.validators.len())
        .find(|idx| *idx != primary_index)
        .unwrap();
    let response = build_signed(
        TestContext::HEIGHT,
        &ctx.priv_keys[responder_index],
        ValidatorId(responder_index as u16),
        ViewNumber::ZERO,
        ConsensusMessage::PrepareResponse {
            proposal_hash: proposal,
        },
    );
    ctx.engine.process_message(response).unwrap();

    persist_engine(&store, key, &ctx.engine).unwrap();

    let restored = load_engine(&store, ctx.validators.clone(), key)
        .unwrap()
        .expect("snapshot present");
    assert_eq!(restored.state().height(), TestContext::HEIGHT);
    assert_eq!(restored.state().proposal(), Some(proposal));
    assert_eq!(
        restored.expected_participants(MessageKind::Commit),
        ctx.engine.expected_participants(MessageKind::Commit)
    );

    clear_snapshot(&store, key).unwrap();
    assert!(load_engine(&store, ctx.validators, key).unwrap().is_none());
}

fn build_signed(
    height: u64,
    private: &neo_crypto::ecc256::PrivateKey,
    validator: ValidatorId,
    view: ViewNumber,
    message: ConsensusMessage,
) -> SignedMessage {
    use neo_crypto::ecdsa::SIGNATURE_SIZE;

    let mut signed = SignedMessage::new(
        height,
        view,
        validator,
        message,
        neo_crypto::SignatureBytes([0u8; SIGNATURE_SIZE]),
    );
    let digest = signed.digest();
    signed.signature = private.secp256r1_sign(digest.as_ref()).expect("signature");
    signed
}
