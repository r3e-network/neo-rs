//! Minimal marker traits shared across Neo crates.
//!
//! These decouple higher-level crates from concrete chain types:
//!
//! - `BlockLike`: common block accessors without exposing internal structure

/// Minimal marker traits used to decouple higher-level crates.
pub mod marker_traits;

pub use marker_traits::BlockLike;
