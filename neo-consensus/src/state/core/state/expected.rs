use alloc::vec::Vec;

use hashbrown::HashSet;

use super::ConsensusState;
use crate::{message::MessageKind, validator::ValidatorId};

impl ConsensusState {
    pub fn expected_participants(&self, kind: MessageKind) -> Option<Vec<ValidatorId>> {
        self.expected.get(&kind).cloned()
    }

    pub(crate) fn refresh_expected(&mut self, kind: MessageKind) {
        match kind {
            MessageKind::PrepareRequest => {
                if let Some(primary) = self.validators.primary_id(self.height, self.view) {
                    self.expected
                        .insert(MessageKind::PrepareRequest, vec![primary]);
                }
                if !self.expected.contains_key(&MessageKind::PrepareResponse) {
                    self.expected
                        .insert(MessageKind::PrepareResponse, self.all_validator_ids());
                }
            }
            MessageKind::PrepareResponse => {
                let mut responders = self.participants_for(MessageKind::PrepareResponse);
                responders.sort();
                responders.dedup();
                if responders.len() == self.validators.len() {
                    self.expected.remove(&MessageKind::PrepareResponse);
                } else if !self.expected.contains_key(&MessageKind::PrepareResponse) {
                    self.expected
                        .insert(MessageKind::PrepareResponse, self.all_validator_ids());
                }
                if responders.is_empty() {
                    self.expected.remove(&MessageKind::Commit);
                } else {
                    self.expected.insert(MessageKind::Commit, responders);
                }
            }
            MessageKind::Commit => {
                let committers = self.participants_for(MessageKind::Commit);
                let committers: HashSet<_> = committers.into_iter().collect();
                if let Some(entry) = self.expected.get(&MessageKind::Commit) {
                    if entry.iter().all(|validator| committers.contains(validator)) {
                        self.expected.remove(&MessageKind::Commit);
                    }
                }
            }
            MessageKind::ChangeView => {
                if self.records.get(&MessageKind::ChangeView).is_some() {
                    self.expected
                        .insert(MessageKind::ChangeView, self.all_validator_ids());
                } else {
                    self.expected.remove(&MessageKind::ChangeView);
                }
            }
        }
    }

    pub(crate) fn seed_prepare_request_expectation(&mut self) {
        if let Some(primary) = self.validators.primary_id(self.height, self.view) {
            self.expected
                .insert(MessageKind::PrepareRequest, vec![primary]);
        } else {
            self.expected.remove(&MessageKind::PrepareRequest);
        }
    }

    fn participants_for(&self, kind: MessageKind) -> Vec<ValidatorId> {
        self.records
            .get(&kind)
            .map(|messages| messages.iter().map(|m| m.validator).collect())
            .unwrap_or_default()
    }

    fn all_validator_ids(&self) -> Vec<ValidatorId> {
        self.validators.iter().map(|v| v.id).collect()
    }
}
