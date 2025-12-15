//! Network module for Neo blockchain
//!
//! This module provides network functionality matching the C# Neo.Network namespace.

pub mod error;
pub mod p2p;
pub mod upnp;

// Re-export commonly used types
pub use error::{NetworkError, NetworkResult};
pub use p2p::*;
pub use upnp::UPnP;
// Backwards-compatible alias for the old module name.
pub use upnp as u_pn_p;
