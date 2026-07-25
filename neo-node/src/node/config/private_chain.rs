//! Private-chain specification assembly from a C# `config.json`.
//!
//! MainNet and TestNet are closed protocol identities selected through
//! [`NeoChainSpec::from_network_type`]. A private chain has no built-in
//! identity, so it must supply a complete [`NeoChainSpec`]. This module builds
//! one from the same `ProtocolConfiguration` document `neo-cli` consumes, so a
//! mixed-implementation network can share one canonical protocol file instead
//! of transcribing the standby committee into per-implementation formats.

use std::fs::File;
use std::path::Path;

use anyhow::Context;
use neo_config::{
    GENESIS_NONCE, GENESIS_TIMESTAMP_MS, GenesisConfig, GenesisValidator, NeoChainSpec,
    ProtocolSettings,
};
use neo_primitives::UInt256;

/// Builds a private [`NeoChainSpec`] from a C# `ProtocolConfiguration` file.
///
/// `expected_genesis_hash`, when supplied, is enforced by the chain
/// specification and pins the chain's genesis identity so a committee
/// transcription error fails at boot instead of forking silently.
pub(super) fn load_private_chain_spec(
    name: &str,
    protocol_config: &Path,
    expected_genesis_hash: Option<&str>,
) -> anyhow::Result<NeoChainSpec> {
    let settings = load_protocol_settings(protocol_config)?;
    let committee = read_standby_committee(protocol_config)?;
    let genesis = derive_genesis(&settings, committee)?;

    let expected = expected_genesis_hash
        .map(|hash| {
            UInt256::parse(hash).map_err(|error| {
                anyhow::anyhow!("invalid [network].expected_genesis_hash {hash:?}: {error}")
            })
        })
        .transpose()?;

    NeoChainSpec::private(name, settings, genesis, expected).with_context(|| {
        format!(
            "building private chain specification from {}",
            protocol_config.display()
        )
    })
}

/// Reads `ProtocolSettings` from `path`.
///
/// [`ProtocolSettings::load`] answers a missing file with the bare C# default
/// record, which for a private chain would boot an empty committee on network
/// 0. A private chain's identity comes entirely from this file, so a missing or
/// unreadable one is an error rather than a fallback.
fn load_protocol_settings(path: &Path) -> anyhow::Result<ProtocolSettings> {
    anyhow::ensure!(
        path.exists(),
        "[network].protocol_config {} does not exist; a private chain's identity \
         cannot fall back to a built-in specification",
        path.display()
    );
    let mut file = File::open(path)
        .with_context(|| format!("opening protocol configuration {}", path.display()))?;
    ProtocolSettings::load_from_stream(&mut file)
        .with_context(|| format!("parsing protocol configuration {}", path.display()))
}

/// Re-reads the committee as written so genesis keeps the operator's exact
/// encoding rather than a re-serialized projection of the parsed points.
fn read_standby_committee(path: &Path) -> anyhow::Result<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading protocol configuration {}", path.display()))?;
    let document: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("parsing protocol configuration {}", path.display()))?;
    let section = document
        .get("ProtocolConfiguration")
        .unwrap_or(&document)
        .get("StandbyCommittee")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "protocol configuration {} has no StandbyCommittee",
                path.display()
            )
        })?;
    let entries = section.as_array().ok_or_else(|| {
        anyhow::anyhow!(
            "protocol configuration {} StandbyCommittee must be an array",
            path.display()
        )
    })?;
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            entry.as_str().map(str::to_owned).ok_or_else(|| {
                anyhow::anyhow!("StandbyCommittee entry {index} must be a hex string")
            })
        })
        .collect()
}

/// Derives the genesis configuration from the protocol settings.
///
/// The genesis block's identity is the BFT address of the standby validators
/// plus the deterministic C# timestamp and nonce, so deriving both sides from
/// one settings object makes a mismatched settings/genesis pair
/// unconstructible.
fn derive_genesis(
    settings: &ProtocolSettings,
    committee: Vec<String>,
) -> anyhow::Result<GenesisConfig> {
    let validators_count = usize::try_from(settings.validators_count).with_context(|| {
        format!(
            "ValidatorsCount {} is not a valid committee size",
            settings.validators_count
        )
    })?;
    anyhow::ensure!(
        validators_count > 0 && validators_count <= committee.len(),
        "ValidatorsCount {validators_count} must be between 1 and the {} \
         StandbyCommittee entries",
        committee.len()
    );

    // C# orders validators by the standby committee's declaration order, which
    // is also the dBFT primary rotation order.
    let validators = committee
        .iter()
        .take(validators_count)
        .map(|public_key| GenesisValidator {
            public_key: public_key.clone(),
            name: None,
        })
        .collect();

    Ok(GenesisConfig {
        timestamp: GENESIS_TIMESTAMP_MS,
        nonce: GENESIS_NONCE,
        validators,
        committee,
        distribution: Vec::new(),
        contracts: Vec::new(),
    })
}
