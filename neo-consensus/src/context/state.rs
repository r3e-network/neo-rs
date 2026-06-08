//! Consensus state enumeration.

/// Consensus state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConsensusState {
    /// Initial state, waiting to start
    #[default]
    Initial,
    /// Primary (speaker) mode - proposing blocks
    Primary,
    /// Backup (validator) mode - validating proposals
    Backup,
    /// View changing - requesting view change
    ViewChanging,
    /// Committed - block has been committed
    Committed,
}
