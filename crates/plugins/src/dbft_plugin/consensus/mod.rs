//! Consensus module for DBFT Plugin
//!
//! This module provides the consensus implementations matching the C# Neo.Plugins.DBFTPlugin.Consensus exactly.

pub mod consensus_context;
pub mod consensus_context_get;
pub mod consensus_context_make_payload;
pub mod consensus_service;
pub mod consensus_service_check;
pub mod consensus_service_on_message;

// Re-export commonly used types
pub use consensus_context::ConsensusContext;
pub use consensus_service::ConsensusService;