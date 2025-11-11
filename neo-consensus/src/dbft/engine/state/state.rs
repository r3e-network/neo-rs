use crate::{
    error::ConsensusError,
    message::{ConsensusMessage, SignedMessage},
    state::{ConsensusState, QuorumDecision, SnapshotState},
    validator::ValidatorSet,
};

use super::replay::ReplayResult;

pub struct DbftEngine {
    pub(super) state: ConsensusState,
}

impl DbftEngine {
    pub fn new(state: ConsensusState) -> Self {
        Self { state }
    }

    pub fn from_snapshot(
        validators: ValidatorSet,
        snapshot: SnapshotState,
    ) -> Result<Self, ConsensusError> {
        ConsensusState::from_snapshot(validators, snapshot).map(Self::new)
    }

    pub fn process_message(
        &mut self,
        message: SignedMessage,
    ) -> Result<QuorumDecision, ConsensusError> {
        let kind = message.kind();
        let pending_view = match &message.message {
            ConsensusMessage::ChangeView { new_view, .. } => Some(*new_view),
            _ => None,
        };
        self.verify_signature(&message)?;
        self.state.register_message(message)?;
        match self.state.quorum(kind) {
            QuorumDecision::ViewChange { new_view, missing } => {
                if let Some(target) = pending_view {
                    if target == new_view {
                        self.state.apply_view_change(new_view);
                    }
                } else {
                    self.state.apply_view_change(new_view);
                }
                Ok(QuorumDecision::ViewChange { new_view, missing })
            }
            decision => Ok(decision),
        }
    }

    pub fn replay_messages<I>(&mut self, messages: I) -> Vec<ReplayResult>
    where
        I: IntoIterator<Item = SignedMessage>,
    {
        messages
            .into_iter()
            .map(|m| match self.process_message(m) {
                Ok(decision) => ReplayResult::Applied(decision),
                Err(_) => ReplayResult::Skipped,
            })
            .collect()
    }

    pub fn advance_height(&mut self, new_height: u64) -> Result<(), ConsensusError> {
        self.state.advance_height(new_height)
    }
}
