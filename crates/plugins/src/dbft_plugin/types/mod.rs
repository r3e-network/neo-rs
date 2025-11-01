//! Types module for DBFT Plugin
//!
//! This module provides the type definitions matching the C# Neo.Plugins.DBFTPlugin.Types exactly.

pub mod change_view_reason;
pub mod consensus_message_type;

// Re-export commonly used types
pub use change_view_reason::ChangeViewReason;
pub use consensus_message_type::ConsensusMessageType;
