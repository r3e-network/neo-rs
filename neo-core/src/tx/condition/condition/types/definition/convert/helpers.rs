use alloc::vec::Vec;

use hex::FromHexError;
use neo_crypto::ecc256::PublicKey;

use crate::tx::condition::{WitnessConditionDto, WitnessConditionError};

use super::super::WitnessCondition;

pub(super) fn map_conditions(
    expressions: Vec<WitnessConditionDto>,
) -> Result<Vec<WitnessCondition>, WitnessConditionError> {
    expressions
        .into_iter()
        .map(WitnessCondition::from_dto)
        .collect()
}

pub(super) fn parse_public_key(text: &str) -> Result<PublicKey, WitnessConditionError> {
    let normalized = text
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    let bytes = hex::decode(normalized)
        .map_err(|_| WitnessConditionError::InvalidGroup(FromHexError::InvalidStringLength))?;
    PublicKey::from_sec1_bytes(&bytes)
        .map_err(|_| WitnessConditionError::InvalidGroup(FromHexError::InvalidStringLength))
}
