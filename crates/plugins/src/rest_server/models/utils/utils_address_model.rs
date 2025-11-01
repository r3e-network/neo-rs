// Copyright (C) 2015-2025 The Neo Project.
//
// UtilsAddressModel mirrors Neo.Plugins.RestServer.Models.Utils.UtilsAddressModel.
// It carries a Neo address string for REST responses.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UtilsAddressModel {
    /// Wallet address returned by the REST endpoint (e.g. "NNLi44dJNXtDNSBkofB48aTVYtb1zZrNEs").
    pub address: String,
}

impl UtilsAddressModel {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
        }
    }
}
