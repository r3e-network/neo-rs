//! # neo-wire
//!
//! Canonical home for Neo's P2P wire envelope. Provides:
//!
//! - [`Message`] — the on-the-wire frame (flags | command | var-length payload).
//! - [`MessageHeader`] — the per-message metadata header.
//! - [`MessageCommand`] — the command discriminator (re-export from `neo-p2p`).
//! - [`MessageFlags`] — the per-message flag bitfield (re-export from `neo-p2p`).
//! - [`NetworkMessage`] — the top-level envelope (header + flags + typed payload).
//! - [`ProtocolMessage`] — the strongly-typed payload enum covering every Neo P2P command.
//! - [`MessageCodec`] — Tokio framed codec for splitting a byte stream into `Message` frames
//!   (gated behind the default `codec` feature).
//! - [`capabilities`] — node-capability descriptors used during the version handshake.
//! - [`ChannelsConfig`] — P2P channel configuration (re-export from `neo-p2p`).
//! - [`timeouts`] — P2P time-constants module (re-export from `neo-p2p`).
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. Depends only on:
//!
//! - `neo-primitives` (Layer 0) — for `UInt160` / `UInt256`.
//! - `neo-p2p` (Layer 1) — for the canonical wire-envelope enums and payload data types.
//! - `neo-payloads` (Layer 1) — for `Block`, `Transaction`, `ExtensiblePayload`.
//! - `neo-io` (Layer 0) — for `BinaryWriter` / `MemoryReader` / `Serializable`.
//! - `neo-extensions` (Layer 0) — for LZ4 compression helpers.
//! - `neo-error` (Layer 0) — for shared error types.
//!
//! Must **not** depend on `neo-core` (deleted), `neo-network` (Layer 2),
//! or any stateful runtime crate.

#![doc(html_root_url = "https://docs.rs/neo-wire/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod capabilities;
pub mod error;
pub mod message;
pub mod network_message;
pub mod protocol_message;

#[cfg(feature = "codec")]
pub mod codec;

// Re-exports from neo-p2p (the canonical home of the wire-command and
// flag enums and the channel-config types). Consumers can `use
// neo_wire::*` and get everything they need without depending on
// neo-p2p directly.
pub use neo_p2p::{
    channels_config::ChannelsConfig,
    message_command::MessageCommand,
    message_flags::MessageFlags,
    node_capability_type::NodeCapabilityType,
    timeouts,
};

pub use error::{WireError, WireResult};
pub use message::{Message, COMPRESSION_THRESHOLD, PAYLOAD_MAX_SIZE};
pub use network_message::{MessageHeader, NetworkMessage};
pub use protocol_message::ProtocolMessage;

#[cfg(feature = "codec")]
pub use codec::MessageCodec;
