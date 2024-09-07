use std::collections::{HashMap, HashSet};
use std::time::Duration;
use crate::hardfork::Hardfork;

/// Represents the protocol settings of the NEO system.
#[derive(Clone, Debug)]
pub struct ProtocolSettings {
    /// The magic number of the NEO network.
    pub network: u32,

    /// The address version of the NEO system.
    pub address_version: u8,

    /// The public keys of the standby committee members.
    pub standby_committee: Vec<ECPoint>,

    /// The number of the validators in NEO system.
    pub validators_count: usize,

    /// The default seed nodes list.
    pub seed_list: Vec<String>,

    /// Indicates the time in milliseconds between two blocks.
    pub milliseconds_per_block: u32,

    /// The maximum increment of the Transaction.valid_until_block field.
    pub max_valid_until_block_increment: u32,

    /// Indicates the maximum number of transactions that can be contained in a block.
    pub max_transactions_per_block: u32,

    /// Indicates the maximum number of transactions that can be contained in the memory pool.
    pub memory_pool_max_transactions: usize,

    /// Indicates the maximum number of blocks that can be traced in the smart contract.
    pub max_traceable_blocks: u32,

    /// Sets the block height from which a hardfork is activated.
    pub hardforks: HashMap<Hardfork, u32>,

    /// Indicates the amount of gas to distribute during initialization.
    /// In the unit of datoshi, 1 GAS = 1e8 datoshi
    pub initial_gas_distribution: u64,
}

impl ProtocolSettings {
    /// The number of members of the committee in NEO system.
    pub fn committee_members_count(&self) -> usize {
        self.standby_committee.len()
    }

    /// Indicates the time between two blocks.
    pub fn time_per_block(&self) -> Duration {
        Duration::from_millis(self.milliseconds_per_block as u64)
    }

    /// The public keys of the standby validators.
    pub fn standby_validators(&self) -> Vec<ECPoint> {
        self.standby_committee.iter().take(self.validators_count).cloned().collect()
    }

    /// The default protocol settings for NEO MainNet.
    pub fn default() -> Self {
        Self {
            network: 0,
            address_version: 0x35,
            standby_committee: Vec::new(),
            validators_count: 0,
            seed_list: Vec::new(),
            milliseconds_per_block: 15000,
            max_valid_until_block_increment: 86400000 / 15000,
            max_transactions_per_block: 512,
            memory_pool_max_transactions: 50_000,
            max_traceable_blocks: 2_102_400,
            initial_gas_distribution: 52_000_000_00000000,
            hardforks: Self::ensure_omitted_hardforks(HashMap::new()),
        }
    }

    /// Loads the ProtocolSettings from a configuration file.
    pub fn load(path: &str, optional: bool) -> Result<Self, Box<dyn std::error::Error>> {
        // Implementation for loading from file would go here
        // For this example, we'll return the default settings
        let settings = Self::default();
        Self::check_hardfork(&settings)?;
        Ok(settings)
    }

    /// Explicitly set the height of all old omitted hardforks to 0 for proper is_hardfork_enabled behaviour.
    fn ensure_omitted_hardforks(mut hardforks: HashMap<Hardfork, u32>) -> HashMap<Hardfork, u32> {
        let all_hardforks: Vec<Hardfork> = vec![]; // Populate with all Hardfork variants
        for hf in all_hardforks {
            if !hardforks.contains_key(&hf) {
                hardforks.insert(hf, 0);
            } else {
                break;
            }
        }
        hardforks
    }

    /// Check if the Hardfork configuration is valid
    fn check_hardfork(&self) -> Result<(), Box<dyn std::error::Error>> {
        let all_hardforks: Vec<Hardfork> = vec![]; // Populate with all Hardfork variants
        let mut sorted_hardforks: Vec<&Hardfork> = self.hardforks.keys().collect();
        sorted_hardforks.sort_by_key(|&k| all_hardforks.iter().position(|&r| r == *k).unwrap());

        // Check for continuity in configured hardforks
        for window in sorted_hardforks.windows(2) {
            let current_index = all_hardforks.iter().position(|&r| r == *window[0]).unwrap();
            let next_index = all_hardforks.iter().position(|&r| r == *window[1]).unwrap();
            if next_index - current_index > 1 {
                return Err("Hardfork configuration is not continuous.".into());
            }
        }

        // Check that block numbers are not higher in earlier hardforks than in later ones
        for window in sorted_hardforks.windows(2) {
            if self.hardforks[window[0]] > self.hardforks[window[1]] {
                return Err(format!("The Hardfork configuration for {:?} is greater than for {:?}", window[0], window[1]).into());
            }
        }

        Ok(())
    }

    /// Check if the Hardfork is Enabled
    pub fn is_hardfork_enabled(&self, hardfork: Hardfork, index: u32) -> bool {
        self.hardforks.get(&hardfork).map_or(false, |&height| index >= height)
    }
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self::default()
    }
}
