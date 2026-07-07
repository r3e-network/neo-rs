use crate::hardfork::{Hardfork, HardforkManager};
use neo_crypto::ECPoint;
#[cfg(test)]
use neo_primitives::constants;
use std::collections::{HashMap, hash_map::Entry};
use std::time::Duration;
use thiserror::Error;

/// Error type returned by `ProtocolSettings::load*` and helpers.
///
/// Replaces the previous `Result<_, String>` returns. Covers the three
/// failure modes: I/O (open/read/seek), JSON parsing, and hardfork
/// validation. `From<String>` preserves backward compatibility for
/// the `.map_err(|e| format!(...))?` patterns elsewhere in the file.
#[derive(Debug, Error)]
pub enum ProtocolConfigError {
    /// I/O failure opening / reading / seeking the config stream.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The file contents could not be parsed as JSON.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// The hardfork sequence is invalid (a later hardfork configured
    /// while an earlier one is missing, or a non-monotonic height).
    #[error("Invalid hardfork sequence: {0}")]
    InvalidHardforkSequence(String),

    /// A hardfork name could not be parsed.
    #[error("Invalid hardfork name: {0}")]
    InvalidHardforkName(String),

    /// A standby-committee entry could not be parsed as a public key.
    #[error("Invalid committee entry '{entry}': {reason}")]
    InvalidCommitteeEntry {
        /// The original string that failed to parse.
        entry: String,
        /// Why it failed.
        reason: String,
    },

    /// Catch-all for legacy `format!()`-based messages.
    #[error("{0}")]
    Other(String),
}

impl From<String> for ProtocolConfigError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for ProtocolConfigError {
    fn from(message: &str) -> Self {
        Self::Other(message.to_string())
    }
}

neo_error::impl_error_from_struct!(neo_error::CoreError, ProtocolConfigError => Configuration);

mod load;
mod parse;
mod presets;

/// Represents the protocol settings of the NEO system.
/// Matches C# ProtocolSettings record exactly
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolSettings {
    /// The magic number of the NEO network.
    /// Matches C# Network property
    pub network: u32,

    /// The address version of the NEO system.
    /// Matches C# AddressVersion property
    pub address_version: u8,

    /// The public keys of the standby committee members.
    /// Matches C# StandbyCommittee property
    pub standby_committee: Vec<ECPoint>,

    /// The number of the validators in NEO system.
    /// Matches C# ValidatorsCount property
    pub validators_count: i32,

    /// The default seed nodes list.
    /// Matches C# SeedList property
    pub seed_list: Vec<String>,

    /// Indicates the time in milliseconds between two blocks.
    /// Matches C# MillisecondsPerBlock property
    pub milliseconds_per_block: u32,

    /// The maximum increment of the ValidUntilBlock field.
    /// Matches C# MaxValidUntilBlockIncrement property
    pub max_valid_until_block_increment: u32,

    /// Indicates the maximum number of transactions that can be contained in a block.
    /// Matches C# MaxTransactionsPerBlock property
    pub max_transactions_per_block: u32,

    /// Indicates the maximum size of a block in bytes.
    /// Matches C# MaxBlockSize property
    pub max_block_size: u32,

    /// Indicates the maximum number of transactions that can be contained in the memory pool.
    /// Matches C# MemoryPoolMaxTransactions property
    pub memory_pool_max_transactions: i32,

    /// Indicates the maximum number of blocks that can be traced in the smart contract.
    /// Matches C# MaxTraceableBlocks property
    pub max_traceable_blocks: u32,

    /// Sets the block height from which a hardfork is activated.
    /// Matches C# Hardforks property
    pub hardforks: HashMap<Hardfork, u32>,

    /// Indicates the amount of gas to distribute during initialization.
    /// Matches C# InitialGasDistribution property
    pub initial_gas_distribution: u64,
}

impl ProtocolSettings {
    /// The number of members of the committee in NEO system.
    /// Matches C# CommitteeMembersCount property
    pub fn committee_members_count(&self) -> usize {
        self.standby_committee.len()
    }

    /// Indicates the time between two blocks.
    /// Matches C# TimePerBlock property
    pub fn time_per_block(&self) -> Duration {
        Duration::from_millis(self.milliseconds_per_block as u64)
    }

    /// The public keys of the standby validators.
    /// Matches C# StandbyValidators property
    pub fn standby_validators(&self) -> Vec<ECPoint> {
        self.standby_committee
            .iter()
            .take(self.validators_count as usize)
            .cloned()
            .collect()
    }

    /// Returns whether the provided hardfork is enabled at the given block height.
    /// Mirrors C# `ProtocolSettings.IsHardforkEnabled`.
    pub fn is_hardfork_enabled(&self, hardfork: Hardfork, block_height: u32) -> bool {
        self.hardforks
            .get(&hardfork)
            .map(|&activation_height| block_height >= activation_height)
            .unwrap_or(false)
    }

    /// Returns whether the hardfork is configured (has an activation height defined),
    /// regardless of whether it has been activated yet. Used for native contract state
    /// generation — methods from defined hardforks are included in the contract's
    /// NEF/ABI even before their activation height.
    pub fn is_hardfork_defined(&self, hardfork: Hardfork) -> bool {
        self.hardforks.contains_key(&hardfork)
    }

    /// Ensures omitted hardforks are included.
    /// Matches C# EnsureOmmitedHardforks method
    fn ensure_omitted_hardforks(hardforks: HashMap<Hardfork, u32>) -> HashMap<Hardfork, u32> {
        let mut hardforks = hardforks;
        let mut encountered_configured = false;
        for hardfork in HardforkManager::all() {
            match hardforks.entry(hardfork) {
                Entry::Occupied(_) => encountered_configured = true,
                Entry::Vacant(entry) if !encountered_configured => {
                    entry.insert(0);
                }
                _ => break,
            }
        }
        hardforks
    }

    fn validate_hardfork_sequence(
        hardforks: &HashMap<Hardfork, u32>,
    ) -> Result<(), ProtocolConfigError> {
        let all = HardforkManager::all();
        let mut previous_index: Option<usize> = None;
        let mut previous_height: Option<u32> = None;

        for (index, hardfork) in all.iter().enumerate() {
            if let Some(&height) = hardforks.get(hardfork) {
                if let Some(prev_index) = previous_index {
                    if index - prev_index > 1 {
                        let missing = all[prev_index + 1];
                        return Err(ProtocolConfigError::InvalidHardforkSequence(format!(
                            "Hardfork {:?} is configured while {:?} is missing. Configure every hardfork sequentially.",
                            hardfork, missing
                        )));
                    }
                }

                if let Some(prev_height) = previous_height {
                    if height < prev_height {
                        return Err(ProtocolConfigError::InvalidHardforkSequence(format!(
                            "Hardfork {:?} activates at block {}, which is before previously configured height {}.",
                            hardfork, height, prev_height
                        )));
                    }
                }

                previous_index = Some(index);
                previous_height = Some(height);
            }
        }

        Ok(())
    }
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self::default_settings()
    }
}

#[cfg(test)]
#[path = "../tests/settings/protocol.rs"]
mod tests;
