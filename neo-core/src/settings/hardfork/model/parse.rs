use alloc::string::ToString;
use core::str::FromStr;

use super::{error::HardforkParseError, Hardfork};

impl FromStr for Hardfork {
    type Err = HardforkParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let normalized = trimmed
            .strip_prefix("HF_")
            .or_else(|| trimmed.strip_prefix("hf_"))
            .unwrap_or(trimmed);
        let lowered = normalized.to_ascii_lowercase();
        match lowered.as_str() {
            "aspidochelone" => Ok(Hardfork::Aspidochelone),
            "basilisk" => Ok(Hardfork::Basilisk),
            "cockatrice" => Ok(Hardfork::Cockatrice),
            "domovoi" => Ok(Hardfork::Domovoi),
            "echidna" => Ok(Hardfork::Echidna),
            "faun" => Ok(Hardfork::Faun),
            "gorgon" => Ok(Hardfork::Gorgon),
            _ => Err(HardforkParseError {
                name: trimmed.to_string(),
            }),
        }
    }
}
