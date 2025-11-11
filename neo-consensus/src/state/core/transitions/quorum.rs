use super::super::state::ConsensusState;
use crate::{message::MessageKind, state::QuorumDecision};

impl ConsensusState {
    pub fn tally(&self, kind: MessageKind) -> usize {
        self.records(kind).len()
    }

    pub fn quorum(&mut self, kind: MessageKind) -> QuorumDecision {
        match kind {
            MessageKind::ChangeView => {
                if let Some(target) = self.change_view_target() {
                    if self.tally(kind) >= self.validators.quorum() {
                        let missing = self.missing_validators(kind);
                        self.expected.remove(&MessageKind::ChangeView);
                        return QuorumDecision::ViewChange {
                            new_view: target,
                            missing,
                        };
                    }
                }
                QuorumDecision::Pending
            }
            _ => {
                if let Some(proposal) = self.proposal {
                    if self.tally(kind) >= self.validators.quorum() {
                        let missing = self.missing_validators(kind);
                        if kind == MessageKind::Commit {
                            self.expected.remove(&MessageKind::Commit);
                        }
                        return QuorumDecision::Proposal {
                            kind,
                            proposal,
                            missing,
                        };
                    }
                }
                QuorumDecision::Pending
            }
        }
    }
}
