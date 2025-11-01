//! Messages module for DBFT Plugin
//!
//! This module provides the message implementations matching the C# Neo.Plugins.DBFTPlugin.Messages exactly.

pub mod change_view;
pub mod commit;
pub mod consensus_message;
pub mod prepare_request;
pub mod prepare_response;
pub mod recovery_message;

// Re-export commonly used types
pub use change_view::ChangeView;
pub use commit::Commit;
pub use consensus_message::{
    ConsensusMessageError, ConsensusMessageHeader, ConsensusMessagePayload, ConsensusMessageResult,
};
pub use prepare_request::PrepareRequest;
pub use prepare_response::PrepareResponse;
pub use recovery_message::*;
