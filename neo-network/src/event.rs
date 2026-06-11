//! Network event types.
//!
//! The canonical [`NetworkEvent`] enum is defined in
//! [`neo_runtime::outcome::NetworkEvent`] as part of the Stage A
//! service trait contract. This module re-exports it and provides a
//! local type alias so the rest of `neo-network` can use the
//! unqualified name.

pub use neo_runtime::NetworkEvent as NetworkEventKind;

/// Local alias for [`neo_runtime::NetworkEvent`].
pub type NetworkEvent = NetworkEventKind;
