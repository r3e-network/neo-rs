// Copyright (C) 2015-2025 The Neo Project.
//
// UtilsAddressIsValidModel mirrors Neo.Plugins.RestServer.Models.Utils.UtilsAddressIsValidModel.
// It extends the base address response with a validation flag.

use super::utils_address_model::UtilsAddressModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UtilsAddressIsValidModel {
    /// Address supplied by the caller.
    pub address: String,
    /// Indicates whether the address can be converted to a script hash.
    pub is_valid: bool,
}

impl UtilsAddressIsValidModel {
    pub fn new(address: impl Into<String>, is_valid: bool) -> Self {
        Self {
            address: address.into(),
            is_valid,
        }
    }
}

impl From<UtilsAddressModel> for UtilsAddressIsValidModel {
    fn from(model: UtilsAddressModel) -> Self {
        Self {
            address: model.address,
            is_valid: false,
        }
    }
}
