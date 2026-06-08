//! Minimal marker traits shared across Neo crates.
//!
//! These decouple higher-level crates from concrete chain types:
//!
//! - `NetworkMessage`: command name + serialization for a wire message
//! - `BlockLike`: common block accessors without exposing internal structure

pub use marker_traits::*;

/// Minimal marker traits used to decouple higher-level crates.
pub mod marker_traits;
