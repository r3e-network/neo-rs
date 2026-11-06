// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Hardfork activation management for Neo N3.
//!
//! Hardfork identities are owned by `neo-primitives`, and network activation
//! defaults are owned by `neo-config`. An empty manager disables unregistered
//! forks, unlike the leading-omission normalization in `ProtocolSettings`.
//!
//! ```rust,no_run
//! use neo_protocol::hardfork::{Hardfork, HardforkManager};
//!
//! let manager = HardforkManager::mainnet();
//! let is_enabled = manager.is_enabled(Hardfork::HfBasilisk, 4_200_000);
//! ```

use std::collections::HashMap;

// Re-export Hardfork from neo-primitives (single source of truth)
pub use neo_primitives::{Hardfork, HardforkParseError};

/// Height configured for `fork`, if the config names one.
///
/// The exhaustive match is the sole bridge from `neo_config::HardforkHeights`
/// to typed activation maps. Adding a primitive hardfork requires mapping its
/// corresponding config field here, without duplicating network default tables.
pub(crate) fn hardfork_height(
    heights: &neo_config::HardforkHeights,
    fork: Hardfork,
) -> Option<u32> {
    match fork {
        Hardfork::HfAspidochelone => heights.hf_aspidochelone,
        Hardfork::HfBasilisk => heights.hf_basilisk,
        Hardfork::HfCockatrice => heights.hf_cockatrice,
        Hardfork::HfDomovoi => heights.hf_domovoi,
        Hardfork::HfEchidna => heights.hf_echidna,
        Hardfork::HfFaun => heights.hf_faun,
        Hardfork::HfGorgon => heights.hf_gorgon,
        Hardfork::HfHuyao => heights.hf_huyao,
    }
}

/// Tracks explicitly registered activation heights; absent hardforks are disabled.
#[derive(Debug)]
pub struct HardforkManager {
    hardforks: HashMap<Hardfork, u32>,
}

impl HardforkManager {
    /// Returns every known hardfork in declaration order.
    pub fn all() -> &'static [Hardfork] {
        &Hardfork::ALL
    }

    /// Creates an empty manager. Unregistered hardforks remain disabled at every height.
    pub fn new() -> Self {
        let hardforks = HashMap::new();
        Self { hardforks }
    }

    /// Creates a manager using the single MainNet source in `neo-config`.
    pub fn mainnet() -> Self {
        Self::from_heights(&neo_config::ProtocolSettings::mainnet().hardforks)
    }

    /// Creates a manager using the single TestNet source in `neo-config`.
    pub fn testnet() -> Self {
        Self::from_heights(&neo_config::ProtocolSettings::testnet().hardforks)
    }

    /// Builds a manager from explicitly configured heights, without filling omissions.
    pub(crate) fn from_heights(heights: &neo_config::HardforkHeights) -> Self {
        let mut hardforks = HashMap::new();
        for hardfork in Hardfork::all() {
            if let Some(height) = hardfork_height(heights, hardfork) {
                hardforks.insert(hardfork, height);
            }
        }
        Self { hardforks }
    }

    /// Registers or replaces the height at which a hardfork takes effect.
    pub fn register(&mut self, hardfork: Hardfork, block_height: u32) {
        self.hardforks.insert(hardfork, block_height);
    }

    /// Checks whether a registered hardfork is active at the specified block height.
    pub fn is_enabled(&self, hardfork: Hardfork, block_height: u32) -> bool {
        match self.hardforks.get(&hardfork) {
            Some(&hardfork_height) => block_height >= hardfork_height,
            None => false, // If hardfork isn't specified in configuration, return false
        }
    }

    /// Gets all explicitly configured hardforks.
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
