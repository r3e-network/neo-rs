use alloc::string::String;

use hex::FromHexError;
use neo_crypto::ecc256::KeyError;
use thiserror::Error;

use super::hardfork::{Hardfork, HardforkParseError};

#[derive(Debug, Clone, Error)]
pub enum ProtocolSettingsError {
    #[error("protocol settings: json parse error: {0}")]
    JsonError(String),
    #[error("protocol settings: toml parse error: {0}")]
    TomlError(String),
    #[error("protocol settings: invalid hardfork name: {name}")]
    InvalidHardforkName { name: String },
    #[error(
        "protocol settings: hardfork configuration has a gap between {current:?} and {next:?}"
    )]
    HardforkGap { current: Hardfork, next: Hardfork },
    #[error(
        "protocol settings: hardfork {next:?} height {next_height} is lower than {current:?} height {current_height}"
    )]
    HardforkHeightRegression {
        current: Hardfork,
        current_height: u32,
        next: Hardfork,
        next_height: u32,
    },
    #[error("protocol settings: standby committee is empty")]
    EmptyCommittee,
    #[error("protocol settings: invalid standby committee hex at index {index}: {source}")]
    InvalidCommitteeHex {
        index: usize,
        #[source]
        source: FromHexError,
    },
    #[error("protocol settings: invalid standby committee key at index {index}: {source}")]
    InvalidCommitteeKey {
        index: usize,
        #[source]
        source: KeyError,
    },
    #[error("protocol settings: validators count {requested} exceeds committee size {available}")]
    ValidatorsExceedCommittee { requested: usize, available: usize },
    #[error("protocol settings: invalid scrypt parameter {param}")]
    InvalidScryptParameter { param: &'static str },
}

impl From<HardforkParseError> for ProtocolSettingsError {
    fn from(err: HardforkParseError) -> Self {
        ProtocolSettingsError::InvalidHardforkName { name: err.name }
    }
}
