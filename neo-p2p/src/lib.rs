//! P2P networking primitives for the Neo N3 Rust node.
//!
//! This crate focuses on protocol message definitions, the binary codec, and
//! a lightweight handshake state machine. IO integration happens in higher
//! level crates. Design notes live in `docs/specs/neo-modules.md#neo-p2p`.

pub mod codec;
pub mod handshake;
pub mod message;
pub mod peer;

pub use codec::NeoMessageCodec;
pub use handshake::{build_version_payload, HandshakeError, HandshakeMachine, HandshakeRole};
pub use message::{Message, VersionPayload};
pub use peer::{Peer, PeerEvent};
