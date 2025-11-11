use alloc::string::String;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
#[error("invalid hardfork name: {name}")]
pub struct HardforkParseError {
    pub name: String,
}
