//! # neo-config::settings
//!
//! Protocol settings, hardfork gates, and node configuration records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-config`. This configuration crate owns typed
//! settings and must not open storage, start services, or run protocol
//! workflows.
//!
//! ## Contents
//!
//! - `hardfork`: hardfork activation identifiers.
//! - `protocol`: Protocol enums, versioned records, and chain-level domain
//!   constants.

pub mod hardfork;
pub mod protocol;

pub use hardfork::{Hardfork, HardforkManager, HardforkParseError};
pub use protocol::ProtocolSettings;
