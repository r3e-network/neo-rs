//! Protocol settings.
//!
//! The canonical typed `ProtocolSettings` now lives in the lower-layer
//! `neo-config` crate (its single source of truth). This module re-exports it so
//! existing `neo_core::protocol_settings::ProtocolSettings` paths keep working.

pub use neo_config::ProtocolSettings;
