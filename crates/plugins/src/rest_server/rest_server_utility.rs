// Copyright (C) 2015-2025 The Neo Project.
//
// RestServerUtility ports the helper functions from Neo.Plugins.RestServer.
// At present we provide the script hash/address conversion helpers required by
// the UtilsController; additional helpers will be added as the remaining
// controllers are ported.

use neo_core::neo_system::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::UInt160;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RestServerUtilityError {
    #[error("Invalid address format: {0}")]
    InvalidAddress(String),
}

pub struct RestServerUtility;

impl RestServerUtility {
    /// Converts a textual representation (address or script hash) into a `UInt160`.
    /// Mirrors `ConvertToScriptHash` from the C# utility and propagates parsing errors.
    pub fn convert_to_script_hash(
        address: &str,
        settings: &ProtocolSettings,
    ) -> Result<UInt160, RestServerUtilityError> {
        if let Ok(hash) = address.parse::<UInt160>() {
            return Ok(hash);
        }

        WalletHelper::to_script_hash(address, settings.address_version).map_err(|err| {
            RestServerUtilityError::InvalidAddress(err)
        })
    }

    /// Attempts to convert the supplied value into a script hash, returning `None` when parsing fails.
    /// Mirrors `TryConvertToScriptHash` from the C# utility.
    pub fn try_convert_to_script_hash(
        address: &str,
        settings: &ProtocolSettings,
    ) -> Option<UInt160> {
        match Self::convert_to_script_hash(address, settings) {
            Ok(hash) => Some(hash),
            Err(_) => None,
        }
    }
}
