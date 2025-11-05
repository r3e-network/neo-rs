#[cfg(feature = "store")]
use neo_store::{Column, ColumnId, Store, StoreExt};

#[cfg(feature = "store")]
use crate::{
    error::ConsensusError, state::ConsensusState, DbftEngine, SnapshotState, ValidatorSet,
};

#[cfg(feature = "store")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "store")]
use thiserror::Error;

#[cfg(feature = "store")]
#[derive(Clone, Copy, Debug)]
pub struct ConsensusColumn;

#[cfg(feature = "store")]
impl Column for ConsensusColumn {
    const ID: ColumnId = ColumnId::new("consensus.snapshot");
}

#[cfg(feature = "store")]
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    neo_base::NeoEncodeDerive,
    neo_base::NeoDecodeDerive,
    Serialize,
    Deserialize,
)]
pub struct SnapshotKey {
    pub network: u32,
}

#[cfg(feature = "store")]
#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("store error: {0}")]
    Store(#[from] neo_store::StoreError),

    #[error("invalid snapshot: {0}")]
    Snapshot(#[from] ConsensusError),
}

#[cfg(feature = "store")]
pub fn persist_engine<S: Store + ?Sized>(
    store: &S,
    key: SnapshotKey,
    engine: &DbftEngine,
) -> Result<(), PersistenceError> {
    let snapshot = engine.snapshot();
    store
        .put_encoded(ConsensusColumn::ID, &key, &snapshot)
        .map_err(PersistenceError::from)
}

#[cfg(feature = "store")]
pub fn load_engine<S: Store + ?Sized>(
    store: &S,
    validators: ValidatorSet,
    key: SnapshotKey,
) -> Result<Option<DbftEngine>, PersistenceError> {
    match store.get_decoded::<SnapshotKey, SnapshotState>(ConsensusColumn::ID, &key)? {
        Some(snapshot) => {
            let state = ConsensusState::from_snapshot(validators, snapshot)?;
            Ok(Some(DbftEngine::new(state)))
        }
        None => Ok(None),
    }
}

#[cfg(feature = "store")]
pub fn clear_snapshot<S: Store + ?Sized>(
    store: &S,
    key: SnapshotKey,
) -> Result<(), PersistenceError> {
    store
        .delete_encoded::<SnapshotKey>(ConsensusColumn::ID, &key)
        .map_err(PersistenceError::from)
}

#[cfg(all(test, feature = "store"))]
mod tests {
    use super::*;
    use alloc::format;
    use crate::{
        message::{ConsensusMessage, MessageKind, ViewNumber},
        validator::{Validator, ValidatorId},
    };
    use neo_base::hash::Hash256;
    use neo_crypto::{ecc256::PrivateKey, Keypair, Secp256r1Sign, SignatureBytes};
    use neo_store::MemoryStore;
    use rand::{rngs::StdRng, RngCore, SeedableRng};

    const HEIGHT: u64 = 42;
    const NETWORK: u32 = 5195086;

    fn build_engine() -> (DbftEngine, Vec<PrivateKey>, ValidatorSet) {
        let mut privs = Vec::new();
        let mut validators = Vec::new();
        let mut rng = StdRng::seed_from_u64(7);
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
        let set = ValidatorSet::new(validators);
        let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
        (DbftEngine::new(state), privs, set)
    }

    #[test]
    fn persist_and_restore_snapshot() {
        let store = MemoryStore::new();
        let key = SnapshotKey { network: NETWORK };

        let (mut engine, privs, validators) = build_engine();
        let primary = validators.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
        let primary_index = validators.index_of(primary).unwrap();
        let proposal = Hash256::new([0xAA; 32]);
        let request = build_signed(
            HEIGHT,
            &privs[primary_index],
            primary,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareRequest {
                proposal_hash: proposal,
                height: HEIGHT,
                tx_hashes: vec![],
            },
        );
        engine.process_message(request).unwrap();

        let responder_index = (0..validators.len())
            .find(|idx| *idx != primary_index)
            .unwrap();
        let response = build_signed(
            HEIGHT,
            &privs[responder_index],
            ValidatorId(responder_index as u16),
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(response).unwrap();

        persist_engine(&store, key, &engine).unwrap();

        let restored = load_engine(&store, validators.clone(), key)
            .unwrap()
            .expect("snapshot present");
        assert_eq!(restored.state().height(), HEIGHT);
        assert_eq!(restored.state().proposal(), Some(proposal));
        assert_eq!(
            restored.expected_participants(MessageKind::Commit),
            engine.expected_participants(MessageKind::Commit)
        );

        clear_snapshot(&store, key).unwrap();
        assert!(load_engine(&store, validators, key).unwrap().is_none());
    }

    fn build_signed(
        height: u64,
        private: &PrivateKey,
        validator: ValidatorId,
        view: ViewNumber,
        message: ConsensusMessage,
    ) -> crate::message::SignedMessage {
        use crate::message::SignedMessage;
        use neo_crypto::ecdsa::SIGNATURE_SIZE;

        let mut signed = SignedMessage::new(
            height,
            view,
            validator,
            message,
            SignatureBytes([0u8; SIGNATURE_SIZE]),
        );
        let digest = signed.digest();
        signed.signature = private.secp256r1_sign(digest.as_ref()).expect("signature");
        signed
    }
}
