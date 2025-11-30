//! Types module for DBFT Plugin
//!
//! This module provides the type definitions matching the C# Neo.Plugins.DBFTPlugin.Types exactly.
//!
//! Types are now defined in the neo-consensus crate and re-exported here for backward compatibility.

// Re-export submodules for backward compatibility with existing import paths
pub mod change_view_reason {
    //! Change view reason re-exports from neo-consensus.
    pub use neo_consensus::ChangeViewReason;
}

pub mod consensus_message_type {
    //! Consensus message type re-exports from neo-consensus.
    pub use neo_consensus::ConsensusMessageType;
}

// Also re-export at module level for convenience
pub use change_view_reason::ChangeViewReason;
pub use consensus_message_type::ConsensusMessageType;
