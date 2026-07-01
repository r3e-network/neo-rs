//! # neo-native-contracts::text
//!
//! Text segmentation and compatibility helpers for native contracts.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `dotnet_graphemes`: .NET grapheme segmentation compatibility.
//! - `dotnet_text_segmentation`: .NET text segmentation compatibility.

pub(crate) mod dotnet_graphemes;
pub(crate) mod dotnet_text_segmentation;
