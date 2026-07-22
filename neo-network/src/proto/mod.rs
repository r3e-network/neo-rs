//! # neo-network::proto
//!
//! P2P command, flag, and channel definitions.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. This service crate owns P2P transport
//! and peer behavior and must not execute blocks, own consensus rules, or
//! mutate storage directly.
//!
//! ## Contents
//!
//! - `channels_config`: P2P channel configuration records.
//! - `message_command`: P2P message command identifiers.
//! - `message_flags`: P2P message flag records.

/// Channel configuration for P2P node bootstrap.
pub mod channels_config;

/// P2P message command identifiers.
pub mod message_command;

/// Message header flags.
pub mod message_flags;

pub use channels_config::ChannelsConfig;
pub use message_command::{MessageCommand, MessageCommandParseError};
pub use message_flags::MessageFlags;
