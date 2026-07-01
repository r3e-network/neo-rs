//! # neo-network::wire
//!
//! Wire encoders, decoders, and deterministic network framing helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. This service crate owns P2P transport
//! and peer behavior and must not execute blocks, own consensus rules, or
//! mutate storage directly.
//!
//! ## Contents
//!
//! - `capabilities`: Node capability records advertised on the P2P wire.
//! - `error`: Typed error definitions and conversions.
//! - `message`: P2P message records and validation helpers.
//! - `network_message`: Network message envelope codec and validation helpers.
//! - `protocol_message`: Protocol message payload traits and routing helpers.
//! - `codec`: Deterministic byte codecs and compression helpers used by Neo
//!   wire data.

pub mod capabilities;
pub mod error;
pub mod message;
pub mod network_message;
pub mod protocol_message;

pub mod codec;

// Re-exports from the local `proto` submodule (the canonical home of the
// wire-command and flag enums and the channel-config types). Consumers can
// `use neo_network::wire::*` and get everything they need without depending
// on any other crate for these primitives.
pub use crate::proto::{
    channels_config::ChannelsConfig, message_command::MessageCommand, message_flags::MessageFlags,
    node_capability_type::NodeCapabilityType, timeouts,
};

pub use error::{WireError, WireResult};
pub use message::{Message, PAYLOAD_MAX_SIZE};
pub use network_message::{MessageHeader, NetworkMessage};
pub use protocol_message::ProtocolMessage;

pub use codec::MessageCodec;
