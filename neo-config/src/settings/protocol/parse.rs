//! JSON and raw configuration parsing for `ProtocolSettings`.
//!
//! Loading bytes from files/streams is handled in `load.rs`; this module owns
//! the C#-compatible `ProtocolConfiguration` projection and committee key
//! parsing before handing hardfork ordering to the root validation helper.

use std::collections::HashMap;
use std::str::FromStr;

use neo_crypto::ECPoint;
use serde::Deserialize;
use serde_json::Value;

use crate::hardfork::Hardfork;

use super::{ProtocolConfigError, ProtocolSettings};

impl ProtocolSettings {
    pub(super) fn from_value(value: Value) -> Result<Self, ProtocolConfigError> {
        if value.is_null() {
            return Ok(Self::csharp_default());
        }

        let section = match value {
            Value::Object(mut map) => map
                .remove("ProtocolConfiguration")
                .unwrap_or(Value::Object(map)),
            other => other,
        };

        let raw: ProtocolConfiguration = serde_json::from_value(section)?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: ProtocolConfiguration) -> Result<Self, ProtocolConfigError> {
        let mut settings = Self::csharp_default();

        if let Some(network) = raw.network {
            settings.network = network;
        }
        if let Some(version) = raw.address_version {
            settings.address_version = version;
        }
        if let Some(validators) = raw.validators_count {
            settings.validators_count = validators;
        }
        if let Some(list) = raw.seed_list {
            settings.seed_list = list;
        }
        if let Some(ms_per_block) = raw.milliseconds_per_block {
            settings.milliseconds_per_block = ms_per_block;
        }
        if let Some(max_tx) = raw.max_transactions_per_block {
            settings.max_transactions_per_block = max_tx;
        }
        if let Some(max_valid) = raw.max_valid_until_block_increment {
            settings.max_valid_until_block_increment = max_valid;
        }
        if let Some(max_mempool) = raw.memory_pool_max_transactions {
            settings.memory_pool_max_transactions = max_mempool;
        }
        if let Some(max_traceable) = raw.max_traceable_blocks {
            settings.max_traceable_blocks = max_traceable;
        }
        if let Some(initial_gas) = raw.initial_gas_distribution {
            settings.initial_gas_distribution = initial_gas;
        }

        if let Some(committee) = raw.standby_committee {
            settings.standby_committee = CommitteeParser::parse_committee(committee)?;
        }

        if let Some(hardforks) = raw.hardforks {
            let mut parsed = HashMap::new();
            for (name, height) in hardforks {
                let hardfork = Hardfork::from_str(&name).map_err(|err| {
                    ProtocolConfigError::InvalidHardforkName(format!("{name}: {err}"))
                })?;
                parsed.insert(hardfork, height);
            }

            settings.hardforks = Self::ensure_omitted_hardforks(parsed);
            Self::validate_hardfork_sequence(&settings.hardforks)?;
        }

        Ok(settings)
    }
}

pub(super) struct CommitteeParser;

impl CommitteeParser {
    pub(super) fn parse_committee_slice(
        entries: &[&str],
    ) -> Result<Vec<ECPoint>, ProtocolConfigError> {
        CommitteeParser::parse_committee(entries.iter().map(|entry| entry.to_string()).collect())
    }

    fn parse_committee(entries: Vec<String>) -> Result<Vec<ECPoint>, ProtocolConfigError> {
        let mut committee = Vec::with_capacity(entries.len());
        for entry in entries {
            let trimmed = neo_primitives::strip_hex_prefix(entry.trim());
            if trimmed.is_empty() {
                continue;
            }
            let bytes = neo_primitives::hex_util::decode_hex(trimmed).map_err(|err| {
                ProtocolConfigError::InvalidCommitteeEntry {
                    entry: entry.clone(),
                    reason: format!("invalid hex: {err}"),
                }
            })?;
            let point = ECPoint::from_bytes(&bytes).map_err(|e| {
                ProtocolConfigError::InvalidCommitteeEntry {
                    entry: entry.clone(),
                    reason: e.to_string(),
                }
            })?;
            committee.push(point);
        }
        Ok(committee)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ProtocolConfiguration {
    #[serde(default)]
    network: Option<u32>,
    #[serde(default)]
    address_version: Option<u8>,
    #[serde(default)]
    standby_committee: Option<Vec<String>>,
    #[serde(default)]
    validators_count: Option<i32>,
    #[serde(default)]
    seed_list: Option<Vec<String>>,
    #[serde(default)]
    milliseconds_per_block: Option<u32>,
    #[serde(default)]
    max_valid_until_block_increment: Option<u32>,
    #[serde(default)]
    max_transactions_per_block: Option<u32>,
    #[serde(default)]
    memory_pool_max_transactions: Option<i32>,
    #[serde(default)]
    max_traceable_blocks: Option<u32>,
    #[serde(default)]
    hardforks: Option<HashMap<String, u32>>,
    #[serde(default)]
    initial_gas_distribution: Option<u64>,
}
