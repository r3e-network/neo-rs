use super::*;

pub(super) fn change_view_msg(new_view: ViewNumber, reason: ChangeViewReason) -> ConsensusMessage {
    ConsensusMessage::ChangeView {
        new_view,
        reason,
        timestamp_ms: 0,
    }
}

pub(super) fn generate_validators() -> (ValidatorSet, Vec<PrivateKey>) {
    generate_validators_with_count(4)
}

pub(super) fn generate_validators_with_count(count: u16) -> (ValidatorSet, Vec<PrivateKey>) {
    let mut privs = Vec::new();
    let mut validators = Vec::new();
    let mut rng = StdRng::seed_from_u64(1337);
    for idx in 0..count {
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

pub(super) fn build_signed(
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
