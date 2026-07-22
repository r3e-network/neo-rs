use crate::hardfork::{Hardfork, HardforkSchedule};
use neo_crypto::Crypto;
use neo_crypto::ECPoint;
use neo_primitives::UInt256;
#[cfg(test)]
use neo_primitives::constants;
use std::time::Duration;
use thiserror::Error;

/// Error type returned by `ProtocolSettings::load*` and helpers.
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
}

neo_error::impl_error_from_struct!(neo_error::CoreError, ProtocolConfigError => Configuration);

mod load;
mod parse;
mod presets;

/// Consensus and chain-execution fields from Neo C# `ProtocolSettings`.
///
/// Operator-owned resource policy is deliberately excluded. For example,
/// transaction-pool capacity is owned by `neo-mempool::TxPoolConfig` and cannot
/// mutate chain identity.
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

    /// Indicates the maximum number of blocks that can be traced in the smart contract.
    /// Matches C# MaxTraceableBlocks property
    pub max_traceable_blocks: u32,

    /// Sets the block height from which a hardfork is activated.
    /// Matches C# Hardforks property
    pub hardforks: HardforkSchedule,

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
        self.hardforks.is_active(hardfork, block_height)
    }

    /// Returns whether the hardfork is configured (has an activation height defined),
    /// regardless of whether it has been activated yet. Used for native contract state
    /// generation — methods from defined hardforks are included in the contract's
    /// NEF/ABI even before their activation height.
    pub fn is_hardfork_defined(&self, hardfork: Hardfork) -> bool {
        self.hardforks.is_defined(hardfork)
    }

    /// Stable digest of every consensus/execution setting represented here.
    ///
    /// This is used to bind asynchronous verification receipts to the exact
    /// protocol schedule. Operator policy and chain metadata such as the
    /// genesis name are intentionally outside this digest.
    #[must_use]
    pub fn identity_digest(&self) -> UInt256 {
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(&self.network.to_le_bytes());
        bytes.push(self.address_version);
        bytes.extend_from_slice(&self.validators_count.to_le_bytes());
        bytes.extend_from_slice(&self.milliseconds_per_block.to_le_bytes());
        bytes.extend_from_slice(&self.max_valid_until_block_increment.to_le_bytes());
        bytes.extend_from_slice(&self.max_transactions_per_block.to_le_bytes());
        bytes.extend_from_slice(&self.max_block_size.to_le_bytes());
        bytes.extend_from_slice(&self.max_traceable_blocks.to_le_bytes());
        bytes.extend_from_slice(&self.initial_gas_distribution.to_le_bytes());
        bytes.extend_from_slice(&(self.standby_committee.len() as u32).to_le_bytes());
        for key in &self.standby_committee {
            let key_bytes = key.as_bytes();
            bytes.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
            bytes.extend_from_slice(key_bytes);
        }
        for hardfork in Hardfork::ALL {
            if let Some(height) = self.hardforks.activation_height(hardfork) {
                bytes.push(1);
                bytes.extend_from_slice(&height.to_le_bytes());
            } else {
                bytes.push(0);
            }
        }
        UInt256::from(Crypto::sha256(&bytes))
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
