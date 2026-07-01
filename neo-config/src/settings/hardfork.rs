// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Hardfork configuration and management for Neo N3.
//!
//! This module provides hardfork activation tracking and management, matching
//! the C# Neo implementation's `ProtocolSettings.Hardforks` behavior.
//!
//! ## Hardforks
//!
//! Neo N3 uses named hardforks for protocol upgrades:
//!
//! | Hardfork | Description |
//! |----------|-------------|
//! | `HfAspidochelone` | First hardfork - basic improvements |
//! | `HfBasilisk` | Second hardfork - enhanced features |
//! | `HfCockatrice` | Third hardfork - protocol optimizations |
//! | `HfDomovoi` | Fourth hardfork - consensus improvements |
//! | `HfEchidna` | Fifth hardfork - VM upgrades |
//! | `HfFaun` | Sixth hardfork - additional features |
//! | `HfGorgon` | Seventh hardfork - latest protocol |
//!
//! ## Usage
//!
//! ```rust,no_run
//! use neo_config::hardfork::{Hardfork, HardforkManager};
//!
//! // Check if a hardfork is enabled at a specific block height
//! let manager = HardforkManager::mainnet();
//! let is_enabled = manager.is_enabled(Hardfork::HfBasilisk, 4_200_000);
//! ```
//!
//! The `Hardfork` enum is defined in [`neo_primitives`] and re-exported here.
//! This module provides the `HardforkManager` for managing hardfork activation heights.

use std::collections::HashMap;

// Re-export Hardfork from neo-primitives (single source of truth)
pub use neo_primitives::{Hardfork, HardforkParseError};

/// Hardfork manager for Neo blockchain (matches C# ProtocolSettings.Hardforks exactly).
#[derive(Debug)]
pub struct HardforkManager {
    hardforks: HashMap<Hardfork, u32>,
}

impl HardforkManager {
    /// Returns every known hardfork in declaration order.
    pub const fn all() -> [Hardfork; 7] {
        Hardfork::all()
    }

    /// Creates a new HardforkManager with default hardfork heights (matches C# ProtocolSettings.Default exactly).
    ///
    /// # Returns
    ///
    /// A new HardforkManager instance.
    pub fn new() -> Self {
        let hardforks = HashMap::new();
        Self { hardforks }
    }

    /// Creates a new HardforkManager with MainNet hardfork heights (matches C# config.mainnet.json exactly).
    pub fn mainnet() -> Self {
        let mut hardforks = HashMap::new();
        hardforks.insert(Hardfork::HfAspidochelone, 1730000);
        hardforks.insert(Hardfork::HfBasilisk, 4120000);
        hardforks.insert(Hardfork::HfCockatrice, 5450000);
        hardforks.insert(Hardfork::HfDomovoi, 5570000);
        hardforks.insert(Hardfork::HfEchidna, 7300000);
        hardforks.insert(Hardfork::HfFaun, 8800000);
        // HfGorgon is defined in the Hardfork enum but is not scheduled in
        // Neo v3.10.0 MainNet config.
        // See: neo-node v3.10.0 src/Neo.CLI/config.mainnet.json
        Self { hardforks }
    }

    /// Creates a new HardforkManager with TestNet hardfork heights (matches C# config.testnet.json exactly).
    pub fn testnet() -> Self {
        let mut hardforks = HashMap::new();
        hardforks.insert(Hardfork::HfAspidochelone, 210000);
        hardforks.insert(Hardfork::HfBasilisk, 2680000);
        hardforks.insert(Hardfork::HfCockatrice, 3967000);
        hardforks.insert(Hardfork::HfDomovoi, 4144000);
        hardforks.insert(Hardfork::HfEchidna, 5870000);
        hardforks.insert(Hardfork::HfFaun, 12960000);
        // HfGorgon is defined in the Hardfork enum but is not scheduled in
        // Neo v3.10.0 TestNet config.
        // See: neo-node v3.10.0 src/Neo.CLI/config.testnet.json
        Self { hardforks }
    }

    /// Registers a hardfork (matches C# ProtocolSettings hardfork registration exactly).
    ///
    /// # Arguments
    ///
    /// * `hardfork` - The hardfork to register.
    /// * `block_height` - The block height at which the hardfork takes effect.
    pub fn register(&mut self, hardfork: Hardfork, block_height: u32) {
        self.hardforks.insert(hardfork, block_height);
    }

    /// Checks if a hardfork is active at the specified block height (matches C# ProtocolSettings.IsHardforkEnabled exactly).
    ///
    /// # Arguments
    ///
    /// * `hardfork` - The hardfork to check.
    /// * `block_height` - The block height to check.
    ///
    /// # Returns
    ///
    /// A boolean indicating whether the hardfork is active.
    pub fn is_enabled(&self, hardfork: Hardfork, block_height: u32) -> bool {
        match self.hardforks.get(&hardfork) {
            Some(&hardfork_height) => block_height >= hardfork_height,
            None => false, // If hardfork isn't specified in configuration, return false
        }
    }

    /// Gets all configured hardforks (matches C# ProtocolSettings.Hardforks property exactly).
    pub fn get_hardforks(&self) -> &HashMap<Hardfork, u32> {
        &self.hardforks
    }
}

impl Default for HardforkManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../tests/settings/hardfork.rs"]
mod tests;
