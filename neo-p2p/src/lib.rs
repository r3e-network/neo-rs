//! P2P networking layer for the node runtime (extracted from
//! `neo-system::network`, Phase 1 S1 of the crate-architecture refactor).
//!
//! The [`p2p`] module holds the `LocalNode`, `RemoteNode`, `TaskManager` and
//! `TaskSession` actors plus the wire vocabulary they orchestrate (payloads,
//! messages, capabilities, connection handling, timeouts). The vocabulary is
//! re-exported at the crate root so historical `network::*` paths keep
//! resolving through the `neo_system::network` re-export shim.
//!
//! System access is dependency-inverted through [`p2p::P2PContext`]: the
//! orchestration runtime implements the trait for its own context type, so
//! this crate never names the system crate in production code.

pub mod p2p;
#[cfg(feature = "upnp")]
pub mod upnp;

pub use p2p::*;
