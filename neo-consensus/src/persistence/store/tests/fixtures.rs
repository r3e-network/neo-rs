use alloc::{format, vec::Vec};

use neo_base::hash::Hash256;
use neo_crypto::{ecc256::PrivateKey, Keypair};
use rand::{rngs::StdRng, RngCore, SeedableRng};

use crate::{
    message::ViewNumber,
    state::ConsensusState,
    validator::{Validator, ValidatorId},
    DbftEngine, ValidatorSet,
};

pub struct TestContext {
    pub engine: DbftEngine,
    pub priv_keys: Vec<PrivateKey>,
    pub validators: ValidatorSet,
}

impl TestContext {
    pub const HEIGHT: u64 = 42;
    pub const NETWORK: u32 = 5_195_086;

    pub fn build() -> Self {
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
        let state = ConsensusState::new(Self::HEIGHT, ViewNumber::ZERO, set.clone());
        Self {
            engine: DbftEngine::new(state),
            priv_keys: privs,
            validators: set,
        }
    }

    pub fn proposal_hash() -> Hash256 {
        Hash256::new([0xAA; 32])
    }
}
