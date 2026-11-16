//! Neo Node Configuration
//!
//! This module provides configuration parsing for the Neo N3 blockchain node.

mod node_config;
mod plugin_settings;
mod sections;

pub use plugin_settings::{resolve_application_logs_store_path, resolve_tokens_tracker_store_path};
pub use sections::*;
// Phase 3 S6a: signer-service config sections sank into neo-wallets::signing;
// re-exported here so every historical `crate::config::*` path stays valid.
pub use neo_wallets::signing::config::*;

#[cfg(test)]
mod tests;
