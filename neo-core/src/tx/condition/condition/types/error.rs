use hex::FromHexError;
use neo_base::encoding::DecodeError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WitnessConditionError {
    #[error("invalid script hash: {0}")]
    InvalidScriptHash(#[from] DecodeError),
    #[error("invalid ECPoint: {0}")]
    InvalidGroup(#[from] FromHexError),
}
