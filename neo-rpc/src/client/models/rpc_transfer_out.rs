// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_transfer_out.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::wallets::helper::Helper as WalletHelper;
use neo_config::ProtocolSettings;
use neo_json::{JObject, JToken};
use neo_primitives::UInt160;
use serde::{Deserialize, Serialize};

/// Transfer output information matching C# RpcTransferOut
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcTransferOut {
    /// Asset hash
    pub asset: UInt160,

    /// Script hash of recipient
    pub script_hash: UInt160,

    /// Transfer value
    pub value: String,
}

impl RpcTransferOut {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
        json.insert("asset".to_string(), JToken::String(self.asset.to_string()));
        json.insert("value".to_string(), JToken::String(self.value.clone()));
        json.insert(
            "address".to_string(),
            JToken::String(WalletHelper::to_address(
                &self.script_hash,
                protocol_settings.address_version,
            )),
        );
        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let asset_str = json
            .get("asset")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'asset' field")?;

        let asset = if asset_str.starts_with("0x") || asset_str.len() == 40 {
            UInt160::parse(&asset_str).map_err(|_| format!("Invalid asset: {}", asset_str))?
        } else {
            WalletHelper::to_script_hash(&asset_str, protocol_settings.address_version)
                .map_err(|_| format!("Invalid asset: {}", asset_str))?
        };

        let value = json
            .get("value")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'value' field")?
            .to_string();

        let address = json
            .get("address")
            .and_then(|v| v.as_string())
            .or_else(|| json.get("scripthash").and_then(|v| v.as_string()))
            .ok_or("Missing or invalid 'address' field")?;

        let script_hash = if address.len() == 40 || address.starts_with("0x") {
            UInt160::parse(&address)
                .map_err(|_| format!("Invalid address or scripthash: {}", address))?
        } else {
            WalletHelper::to_script_hash(&address, protocol_settings.address_version)
                .map_err(|_| format!("Invalid address or scripthash: {}", address))?
        };

        Ok(Self {
            asset,
            script_hash,
            value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_transfer_out_roundtrip() {
        let settings = ProtocolSettings::default_settings();
        let asset = UInt160::parse("0102030405060708090a0b0c0d0e0f1011121314").unwrap();
        let script_hash = UInt160::parse("1112131415161718191a1b1c1d1e1f2021222324").unwrap();

        let transfer = RpcTransferOut {
            asset,
            script_hash,
            value: "42".to_string(),
        };

        let json = transfer.to_json(&settings);
        let parsed = RpcTransferOut::from_json(&json, &settings).expect("parse");

        assert_eq!(parsed.asset, transfer.asset);
        assert_eq!(parsed.script_hash, transfer.script_hash);
        assert_eq!(parsed.value, transfer.value);
        assert_eq!(
            json.get("address").and_then(|t| t.as_string()).unwrap(),
            WalletHelper::to_address(&transfer.script_hash, settings.address_version)
        );
    }

    #[test]
    fn rpc_transfer_out_accepts_address_for_asset() {
        let settings = ProtocolSettings::default_settings();
        let script_hash = UInt160::parse("1112131415161718191a1b1c1d1e1f2021222324").unwrap();
        let mut json = JObject::new();
        let asset_address = WalletHelper::to_address(&UInt160::zero(), settings.address_version);
        json.insert("asset".to_string(), JToken::String(asset_address.clone()));
        json.insert("value".to_string(), JToken::String("1".to_string()));
        json.insert(
            "address".to_string(),
            JToken::String(WalletHelper::to_address(
                &script_hash,
                settings.address_version,
            )),
        );

        let parsed = RpcTransferOut::from_json(&json, &settings).expect("parse");
        assert_eq!(
            parsed.asset,
            WalletHelper::to_script_hash(&asset_address, settings.address_version).unwrap()
        );
        assert_eq!(parsed.script_hash, script_hash);
    }

    #[test]
    fn rpc_transfer_out_accepts_scripthash_field() {
        let asset = UInt160::parse("0102030405060708090a0b0c0d0e0f1011121314").unwrap();
        let script_hash = UInt160::parse("1112131415161718191a1b1c1d1e1f2021222324").unwrap();

        let mut json = JObject::new();
        json.insert("asset".to_string(), JToken::String(asset.to_string()));
        json.insert("value".to_string(), JToken::String("5".to_string()));
        json.insert(
            "scripthash".to_string(),
            JToken::String(script_hash.to_string()),
        );

        let parsed =
            RpcTransferOut::from_json(&json, &ProtocolSettings::default_settings()).expect("parse");
        assert_eq!(parsed.script_hash, script_hash);
    }
}
