use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::message::{ChangeViewReason, MessageKind};
use crate::state::SnapshotState;
use crate::validator::ValidatorId;

use super::state::DbftEngine;

impl DbftEngine {
    pub fn state(&self) -> &crate::state::ConsensusState {
        &self.state
    }

    pub fn snapshot(&self) -> SnapshotState {
        self.state.snapshot()
    }

    pub fn into_state(self) -> crate::state::ConsensusState {
        self.state
    }

    pub fn participation(&self) -> BTreeMap<MessageKind, Vec<ValidatorId>> {
        self.state.participation_by_kind()
    }

    pub fn tallies(&self) -> BTreeMap<MessageKind, usize> {
        self.state.tallies()
    }

    pub fn quorum_threshold(&self) -> usize {
        self.state.quorum_threshold()
    }

    pub fn primary(&self) -> Option<ValidatorId> {
        self.state.primary()
    }

    pub fn change_view_reasons(&self) -> BTreeMap<ValidatorId, ChangeViewReason> {
        self.state.change_view_reasons()
    }

    pub fn change_view_reason_counts(&self) -> BTreeMap<ChangeViewReason, usize> {
        self.state.change_view_reason_counts()
    }

    pub fn change_view_total(&self) -> u64 {
        self.state.change_view_total()
    }

    pub fn missing_validators(&self, kind: MessageKind) -> Vec<ValidatorId> {
        self.state.missing_validators(kind)
    }

    pub fn expected_participants(&self, kind: MessageKind) -> Option<Vec<ValidatorId>> {
        self.state.expected_participants(kind)
    }
}
