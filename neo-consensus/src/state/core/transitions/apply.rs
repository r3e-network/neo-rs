use super::super::state::ConsensusState;
use crate::{error::ConsensusError, message::ViewNumber};

impl ConsensusState {
    pub fn apply_view_change(&mut self, new_view: ViewNumber) {
        self.view = new_view;
        self.records.clear();
        self.proposal = None;
        self.expected.clear();
        self.change_view_reasons.clear();
        self.change_view_reason_counts.clear();
        self.seed_prepare_request_expectation();
    }

    pub fn advance_height(&mut self, new_height: u64) -> Result<(), ConsensusError> {
        if new_height <= self.height {
            return Err(ConsensusError::InvalidHeightTransition {
                current: self.height,
                requested: new_height,
            });
        }
        self.height = new_height;
        self.view = ViewNumber::ZERO;
        self.records.clear();
        self.proposal = None;
        self.expected.clear();
        self.change_view_reasons.clear();
        self.change_view_reason_counts.clear();
        self.seed_prepare_request_expectation();
        Ok(())
    }
}
