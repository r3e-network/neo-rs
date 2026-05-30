//! Hardfork configuration and management for Neo N3.
//!
//! `Hardfork` is defined in [`neo_primitives`]; the `HardforkManager` and the
//! global hardfork helpers now live in the lower-layer `neo-config` crate
//! (alongside the canonical `ProtocolSettings`). This module re-exports them so
//! existing `neo_core::hardfork::*` paths keep working.

pub use neo_config::hardfork::{
    is_hardfork_enabled, Hardfork, HardforkManager, HardforkParseError,
};
