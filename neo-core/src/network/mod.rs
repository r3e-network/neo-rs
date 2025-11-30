//! Network module for Neo blockchain
//!
//! This module provides network functionality matching the C# Neo.Network namespace.

pub mod error;
pub mod p2p;
pub mod u_pn_p;

// Re-export commonly used types
pub use error::{NetworkError, NetworkResult};
pub use p2p::*;
pub use u_pn_p::UPnP;
