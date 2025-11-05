use alloc::{string::String, vec::Vec};

use neo_crypto::ecc256::PublicKey;

use crate::message::ViewNumber;

/// Identifier assigned to each validator position.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    neo_base::NeoEncodeDerive,
    neo_base::NeoDecodeDerive,
)]
pub struct ValidatorId(pub u16);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validator {
    pub id: ValidatorId,
    pub public_key: PublicKey,
    pub alias: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ValidatorSet {
    validators: Vec<Validator>,
}

impl ValidatorSet {
    pub fn new(mut validators: Vec<Validator>) -> Self {
        validators.sort_by_key(|v| v.id);
        Self { validators }
    }

    pub fn len(&self) -> usize {
        self.validators.len()
    }

    pub fn is_empty(&self) -> bool {
        self.validators.is_empty()
    }

    /// Minimum number of matching votes required to reach consensus.
    pub fn quorum(&self) -> usize {
        // classic dBFT quorum: floor(2n/3) + 1
        (self.len() * 2) / 3 + 1
    }

    pub fn get(&self, id: ValidatorId) -> Option<&Validator> {
        self.index_of(id).map(|idx| &self.validators[idx])
    }

    pub fn iter(&self) -> impl Iterator<Item = &Validator> {
        self.validators.iter()
    }

    pub fn index_of(&self, id: ValidatorId) -> Option<usize> {
        self.validators.binary_search_by_key(&id, |v| v.id).ok()
    }

    pub fn primary_id(&self, height: u64, view: ViewNumber) -> Option<ValidatorId> {
        self.primary_for(height, view).map(|v| v.id)
    }

    pub fn primary_for(&self, height: u64, view: ViewNumber) -> Option<&Validator> {
        if self.validators.is_empty() {
            return None;
        }
        let n = self.validators.len() as u64;
        let height_mod = height % n;
        let view_mod = (view.0 as u64) % n;
        let index = (height_mod + view_mod) % n;
        self.validators.get(index as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::{ecc256::PrivateKey, Keypair};

    #[test]
    fn primary_rotates_with_height_and_view() {
        let mut validators = Vec::new();
        for id in 0u16..4 {
            let mut bytes = [0u8; 32];
            bytes[31] = (id + 1) as u8;
            let private = PrivateKey::new(bytes);
            let keypair = Keypair::from_private(private).unwrap();
            validators.push(Validator {
                id: ValidatorId(id),
                public_key: keypair.public_key,
                alias: None,
            });
        }
        let set = ValidatorSet::new(validators);
        let primary0 = set.primary_id(0, ViewNumber::ZERO).unwrap();
        assert_eq!(primary0, ValidatorId(0));
        let primary1 = set.primary_id(1, ViewNumber::ZERO).unwrap();
        assert_eq!(primary1, ValidatorId(1));
        let view1 = set.primary_id(1, ViewNumber(1)).unwrap();
        assert_eq!(view1, ValidatorId(2));
    }
}
