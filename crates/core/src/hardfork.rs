// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// modifications are permitted.

//! Hardfork configuration and detection for Neo blockchain.

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::RwLock;

/// Represents a hardfork in the Neo blockchain (matches C# Hardfork enum exactly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Hardfork {
    /// Aspidochelone hardfork
    HF_Aspidochelone = 0,
    /// Basilisk hardfork
    HF_Basilisk = 1,
    /// Cockatrice hardfork
    HF_Cockatrice = 2,
    /// Domovoi hardfork
    HF_Domovoi = 3,
    /// Echidna hardfork
    HF_Echidna = 4,
}

/// Hardfork manager for Neo blockchain (matches C# ProtocolSettings.Hardforks exactly).
#[derive(Debug)]
pub struct HardforkManager {
    hardforks: HashMap<Hardfork, u32>,
}

lazy_static! {
    static ref INSTANCE: RwLock<HardforkManager> = RwLock::new(HardforkManager::new());
}

impl HardforkManager {
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
        hardforks.insert(Hardfork::HF_Aspidochelone, 1730000);
        hardforks.insert(Hardfork::HF_Basilisk, 4120000);
        hardforks.insert(Hardfork::HF_Cockatrice, 5450000);
        hardforks.insert(Hardfork::HF_Domovoi, 5570000);
        hardforks.insert(Hardfork::HF_Echidna, 7300000);
        Self { hardforks }
    }

    /// Creates a new HardforkManager with TestNet hardfork heights (matches C# config.testnet.json exactly).
    pub fn testnet() -> Self {
        let mut hardforks = HashMap::new();
        hardforks.insert(Hardfork::HF_Aspidochelone, 210000);
        hardforks.insert(Hardfork::HF_Basilisk, 2680000);
        hardforks.insert(Hardfork::HF_Cockatrice, 3967000);
        hardforks.insert(Hardfork::HF_Domovoi, 4144000);
        hardforks.insert(Hardfork::HF_Echidna, 5870000);
        Self { hardforks }
    }

    /// Gets the global instance of the HardforkManager.
    ///
    /// # Returns
    ///
    /// A reference to the global HardforkManager instance.
    pub fn instance() -> &'static RwLock<HardforkManager> {
        &INSTANCE
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
pub fn is_hardfork_enabled(hardfork: Hardfork, block_height: u32) -> bool {
    if let Ok(manager) = HardforkManager::instance().read() {
        manager.is_enabled(hardfork, block_height)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, Transaction, UInt160, UInt256};

    #[test]
    fn test_hardfork_manager() {
        let mut manager = HardforkManager::new();

        assert!(!manager.is_enabled(Hardfork::HF_Aspidochelone, 0));
        assert!(!manager.is_enabled(Hardfork::HF_Basilisk, 0));

        // Register a hardfork
        manager.register(Hardfork::HF_Aspidochelone, 100);

        // Test hardfork activation
        assert!(!manager.is_enabled(Hardfork::HF_Aspidochelone, 99));
        assert!(manager.is_enabled(Hardfork::HF_Aspidochelone, 100));
        assert!(manager.is_enabled(Hardfork::HF_Aspidochelone, 101));

        // Test unregistered hardfork
        assert!(!manager.is_enabled(Hardfork::HF_Basilisk, 1000));
    }

    #[test]
    fn test_mainnet_hardforks() {
        let manager = HardforkManager::mainnet();

        assert!(manager.is_enabled(Hardfork::HF_Aspidochelone, 1730000));
        assert!(!manager.is_enabled(Hardfork::HF_Aspidochelone, 1729999));

        assert!(manager.is_enabled(Hardfork::HF_Basilisk, 4120000));
        assert!(!manager.is_enabled(Hardfork::HF_Basilisk, 4119999));

        assert!(manager.is_enabled(Hardfork::HF_Echidna, 7300000));
        assert!(!manager.is_enabled(Hardfork::HF_Echidna, 7299999));
    }

    #[test]
    fn test_testnet_hardforks() {
        let manager = HardforkManager::testnet();

        assert!(manager.is_enabled(Hardfork::HF_Aspidochelone, 210000));
        assert!(!manager.is_enabled(Hardfork::HF_Aspidochelone, 209999));

        assert!(manager.is_enabled(Hardfork::HF_Basilisk, 2680000));
        assert!(!manager.is_enabled(Hardfork::HF_Basilisk, 2679999));

        assert!(manager.is_enabled(Hardfork::HF_Echidna, 5870000));
        assert!(!manager.is_enabled(Hardfork::HF_Echidna, 5869999));
    }

    #[test]
    fn test_global_hardfork_manager() {
        // This test modifies the global instance, so it should be run in isolation
        if let Ok(mut manager) = HardforkManager::instance().write() {
            // Register a hardfork
            manager.register(Hardfork::HF_Aspidochelone, 300);
        }

        assert!(!is_hardfork_enabled(Hardfork::HF_Aspidochelone, 299));
        assert!(is_hardfork_enabled(Hardfork::HF_Aspidochelone, 300));
        assert!(is_hardfork_enabled(Hardfork::HF_Aspidochelone, 301));
    }
}
