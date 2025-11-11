use alloc::{collections::BTreeMap, vec::Vec};

use hashbrown::HashMap;
use neo_base::hash::Hash256;

use crate::{
    message::{ChangeViewReason, MessageKind, SignedMessage, ViewNumber},
    validator::{ValidatorId, ValidatorSet},
};

pub struct ConsensusState {
    pub(crate) height: u64,
    pub(crate) view: ViewNumber,
    pub(crate) validators: ValidatorSet,
    pub(crate) records: HashMap<MessageKind, Vec<SignedMessage>>,
    pub(crate) proposal: Option<Hash256>,
    pub(crate) expected: HashMap<MessageKind, Vec<ValidatorId>>,
    pub(crate) change_view_reasons: HashMap<ValidatorId, ChangeViewReason>,
    pub(crate) change_view_reason_counts: BTreeMap<ChangeViewReason, usize>,
    pub(crate) change_view_total: u64,
}

impl ConsensusState {
    pub fn new(height: u64, view: ViewNumber, validators: ValidatorSet) -> Self {
        let mut state = Self {
            height,
            view,
            validators,
            records: HashMap::new(),
            proposal: None,
            expected: HashMap::new(),
            change_view_reasons: HashMap::new(),
            change_view_reason_counts: BTreeMap::new(),
            change_view_total: 0,
        };
        state.seed_prepare_request_expectation();
        state
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn view(&self) -> ViewNumber {
        self.view
    }

    pub fn validators(&self) -> &ValidatorSet {
        &self.validators
    }

    pub fn proposal(&self) -> Option<Hash256> {
        self.proposal
    }

    pub fn records(&self, kind: MessageKind) -> &[SignedMessage] {
        self.records
            .get(&kind)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn participation_by_kind(&self) -> BTreeMap<MessageKind, Vec<ValidatorId>> {
        let mut map = BTreeMap::new();
        for (kind, messages) in &self.records {
            let validators = messages.iter().map(|m| m.validator).collect();
            map.insert(*kind, validators);
        }
        map
    }

    pub fn tallies(&self) -> BTreeMap<MessageKind, usize> {
        let mut map = BTreeMap::new();
        for (kind, messages) in &self.records {
            map.insert(*kind, messages.len());
        }
        map
    }

    pub fn quorum_threshold(&self) -> usize {
        self.validators.quorum()
    }

    pub fn primary(&self) -> Option<ValidatorId> {
        self.validators.primary_id(self.height, self.view)
    }
}
