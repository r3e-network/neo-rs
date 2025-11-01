// Copyright (C) 2015-2025 The Neo Project.
//
// UtilsController mirrors Neo.Plugins.RestServer.Controllers.v1.UtilsController.
// It exposes helper endpoints for converting between script hashes and Neo
// addresses and for validating input formats.

use crate::rest_server::binder::uint160_binder_provider::UInt160BinderProvider;
use crate::rest_server::exceptions::address_format_exception::AddressFormatException;
use crate::rest_server::exceptions::node_network_exception::NodeNetworkException;
use crate::rest_server::exceptions::script_hash_format_exception::ScriptHashFormatException;
use crate::rest_server::models::error::error_model::ErrorModel;
use crate::rest_server::models::utils::utils_address_is_valid_model::UtilsAddressIsValidModel;
use crate::rest_server::models::utils::utils_address_model::UtilsAddressModel;
use crate::rest_server::models::utils::utils_script_hash_model::UtilsScriptHashModel;
use crate::rest_server::rest_server_plugin::RestServerGlobals;
use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::NeoSystem;
use std::sync::Arc;

pub struct UtilsController {
    neo_system: Arc<NeoSystem>,
}

impl UtilsController {
    pub fn new() -> Result<Self, ErrorModel> {
        RestServerGlobals::neo_system()
            .map(|system| Self { neo_system: system })
            .ok_or_else(|| NodeNetworkException::new().to_error_model())
    }

    pub fn script_hash_to_wallet_address(
        &self,
        hash: &str,
    ) -> Result<UtilsAddressModel, ErrorModel> {
        let script_hash = UInt160BinderProvider::bind(hash).ok_or_else(|| {
            ScriptHashFormatException::with_message(format!("'{hash}' is invalid."))
                .to_error_model()
        })?;

        let address =
            WalletHelper::to_address(&script_hash, self.neo_system.settings().address_version);
        Ok(UtilsAddressModel::new(address))
    }

    pub fn wallet_address_to_script_hash(
        &self,
        address: &str,
    ) -> Result<UtilsScriptHashModel, ErrorModel> {
        match RestServerUtility::convert_to_script_hash(address, self.neo_system.settings()) {
            Ok(hash) => Ok(UtilsScriptHashModel::new(hash.to_string())),
            Err(RestServerUtilityError::InvalidAddress(message)) => {
                Err(AddressFormatException::with_message(message).to_error_model())
            }
            Err(RestServerUtilityError::StackItem(message)) => {
                Err(AddressFormatException::with_message(message).to_error_model())
            }
        }
    }

    pub fn validate_address(&self, address: &str) -> Result<UtilsAddressIsValidModel, ErrorModel> {
        let is_valid =
            RestServerUtility::try_convert_to_script_hash(address, self.neo_system.settings())
                .is_some();
        Ok(UtilsAddressIsValidModel::new(address, is_valid))
    }
}
