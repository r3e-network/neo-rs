//! Network actor layer for the node runtime.
//!
//! The P2P wire vocabulary (payloads, messages, capabilities, connection
//! handling, timeouts) remains in `neo-core::network`; the actors that
//! orchestrate it live in [`p2p`]. The vocabulary is re-exported from
//! [`p2p`] so the historical `network::*` paths keep resolving.

pub mod p2p;
#[cfg(feature = "upnp")]
pub mod upnp;

pub use p2p::*;
