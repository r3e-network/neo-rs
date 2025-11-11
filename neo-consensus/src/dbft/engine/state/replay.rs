use crate::state::QuorumDecision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayResult {
    Applied(QuorumDecision),
    Skipped,
}
