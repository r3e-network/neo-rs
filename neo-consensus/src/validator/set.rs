use alloc::vec::Vec;

use crate::message::ViewNumber;

use super::{Validator, ValidatorId};

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
