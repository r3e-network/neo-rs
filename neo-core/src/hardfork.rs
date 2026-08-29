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
//! | `HfGorgon` | Seventh hardfork (Neo 3.10) |
//! | `HfHuyao` | Eighth hardfork (Neo 3.10) |
//!
//! ## Usage
//!
//! ```rust,no_run
//! use neo_core::hardfork::{Hardfork, HardforkManager};
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
    pub fn all() -> &'static [Hardfork] {
        &Hardfork::ALL
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

    /// Creates a new HardforkManager with MainNet hardfork heights.
    ///
    /// Heights come from [`neo_config::ProtocolSettings::mainnet`] — the
    /// single source of truth for network parameters. Verified against live
    /// MainNet nodes (`getversion`, Neo 3.10.1): Faun = 8_800_000,
    /// Gorgon = 12_020_000.
    pub fn mainnet() -> Self {
        Self::from_heights(&neo_config::ProtocolSettings::mainnet().hardforks)
    }

    /// Creates a new HardforkManager with TestNet hardfork heights.
    ///
    /// Heights come from [`neo_config::ProtocolSettings::testnet`] — the
    /// single source of truth for network parameters. Verified against live
    /// TestNet nodes (`getversion`, Neo 3.10.1): Faun = 12_960_000,
    /// Gorgon = 17_960_000.
    pub fn testnet() -> Self {
        Self::from_heights(&neo_config::ProtocolSettings::testnet().hardforks)
    }

    /// Builds a manager from the shared `HardforkHeights` configuration; an
    /// unconfigured hardfork (None) is treated as not activated, exactly like
    /// C# `ProtocolSettings.Hardforks`.
    fn from_heights(heights: &neo_config::HardforkHeights) -> Self {
        let mut hardforks = HashMap::new();
        let pairs = [
            (Hardfork::HfAspidochelone, heights.hf_aspidochelone),
            (Hardfork::HfBasilisk, heights.hf_basilisk),
            (Hardfork::HfCockatrice, heights.hf_cockatrice),
            (Hardfork::HfDomovoi, heights.hf_domovoi),
            (Hardfork::HfEchidna, heights.hf_echidna),
            (Hardfork::HfFaun, heights.hf_faun),
            (Hardfork::HfGorgon, heights.hf_gorgon),
            (Hardfork::HfHuyao, heights.hf_huyao),
        ];
        for (hardfork, height) in pairs {
            if let Some(height) = height {
                hardforks.insert(hardfork, height);
            }
        }
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

crate::impl_default_via_new!(HardforkManager);

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_hardfork_manager() {
        let mut manager = HardforkManager::new();
        assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 0));
        assert!(!manager.is_enabled(Hardfork::HfBasilisk, 0));
        // Register a hardfork
        manager.register(Hardfork::HfAspidochelone, 100);
        // Test hardfork activation
        assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 99));
        assert!(manager.is_enabled(Hardfork::HfAspidochelone, 100));
        assert!(manager.is_enabled(Hardfork::HfAspidochelone, 101));
        // Test unregistered hardfork
        assert!(!manager.is_enabled(Hardfork::HfBasilisk, 1000));
    }

    #[test]
    fn test_mainnet_hardforks() {
        let manager = HardforkManager::mainnet();
        assert!(manager.is_enabled(Hardfork::HfAspidochelone, 1730000));
        assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 1729999));
        assert!(manager.is_enabled(Hardfork::HfBasilisk, 4120000));
        assert!(!manager.is_enabled(Hardfork::HfBasilisk, 4119999));
        assert!(manager.is_enabled(Hardfork::HfEchidna, 7300000));
        assert!(!manager.is_enabled(Hardfork::HfEchidna, 7299999));
        assert!(manager.is_enabled(Hardfork::HfFaun, 8800000));
        assert!(!manager.is_enabled(Hardfork::HfFaun, 8799999));
        assert!(manager.is_enabled(Hardfork::HfGorgon, 12020000));
        assert!(!manager.is_enabled(Hardfork::HfGorgon, 12019999));
        assert!(!manager.is_enabled(Hardfork::HfHuyao, u32::MAX));
    }

    #[test]
    fn test_testnet_hardforks() {
        let manager = HardforkManager::testnet();
        assert!(manager.is_enabled(Hardfork::HfAspidochelone, 210000));
        assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 209999));
        assert!(manager.is_enabled(Hardfork::HfBasilisk, 2680000));
        assert!(!manager.is_enabled(Hardfork::HfBasilisk, 2679999));
        assert!(manager.is_enabled(Hardfork::HfEchidna, 5870000));
        assert!(!manager.is_enabled(Hardfork::HfEchidna, 5869999));
        assert!(manager.is_enabled(Hardfork::HfFaun, 12960000));
        assert!(!manager.is_enabled(Hardfork::HfFaun, 12959999));
        assert!(manager.is_enabled(Hardfork::HfGorgon, 17960000));
        assert!(!manager.is_enabled(Hardfork::HfGorgon, 17959999));
        assert!(!manager.is_enabled(Hardfork::HfHuyao, u32::MAX));
    }
}
