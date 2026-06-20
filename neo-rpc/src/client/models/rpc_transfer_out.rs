use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, strip_hex_prefix};
use neo_serialization::json::{JObject, JToken};
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;
use serde::{Deserialize, Serialize};

/// Transfer output information matching C# `RpcTransferOut`
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
    /// Matches C# `ToJson`
    #[must_use]
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
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let asset_str = json
            .get("asset")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'asset' field"))?;

        let asset = if is_hex_or_prefixed_hash(&asset_str) {
            UInt160::parse(&asset_str)
                .map_err(|_| CoreError::other(format!("Invalid asset: {asset_str}")))?
        } else {
            WalletHelper::to_script_hash(&asset_str, protocol_settings.address_version)
                .map_err(|_| CoreError::other(format!("Invalid asset: {asset_str}")))?
        };

        let value = json
            .get("value")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'value' field"))?;

        let address = json
            .get("address")
            .and_then(neo_serialization::json::JToken::as_string)
            .or_else(|| {
                json.get("scripthash")
                    .and_then(neo_serialization::json::JToken::as_string)
            })
            .ok_or_else(|| CoreError::other("Missing or invalid 'address' field"))?;

        let script_hash = if is_hex_or_prefixed_hash(&address) {
            UInt160::parse(&address).map_err(|_| {
                CoreError::other(format!("Invalid address or scripthash: {address}"))
            })?
        } else {
            WalletHelper::to_script_hash(&address, protocol_settings.address_version).map_err(
                |_| CoreError::other(format!("Invalid address or scripthash: {address}")),
            )?
        };

        Ok(Self {
            asset,
            script_hash,
            value,
        })
    }
}

fn is_hex_or_prefixed_hash(value: &str) -> bool {
    value.len() == 40 || strip_hex_prefix(value) != value
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::rpc_case_params;
    use super::*;
    use neo_serialization::json::JArray;

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

        let mut json = transfer.to_json(&settings);
        json.insert(
            "asset".to_string(),
            JToken::String(format!("0X{}", strip_hex_prefix(&asset.to_string()))),
        );
        json.insert(
            "address".to_string(),
            JToken::String(format!("0X{}", strip_hex_prefix(&script_hash.to_string()))),
        );
        let parsed = RpcTransferOut::from_json(&json, &settings).expect("parse");

        assert_eq!(parsed.asset, transfer.asset);
        assert_eq!(parsed.script_hash, transfer.script_hash);
        assert_eq!(parsed.value, transfer.value);
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

    #[test]
    fn transfer_out_to_json_matches_rpc_test_case() {
        let settings = ProtocolSettings::default_settings();
        let Some(params) = rpc_case_params("sendmanyasync") else {
            return;
        };
        let transfers = params
            .get(1)
            .and_then(|value| value.as_array())
            .expect("transfer outputs array");
        let parsed = transfers
            .children()
            .iter()
            .filter_map(|entry| entry.as_ref())
            .filter_map(|token| token.as_object())
            .filter_map(|obj| RpcTransferOut::from_json(obj, &settings).ok())
            .collect::<Vec<_>>();
        let actual = JArray::from(
            parsed
                .iter()
                .map(|transfer| JToken::Object(transfer.to_json(&settings)))
                .collect::<Vec<_>>(),
        );
        assert_eq!(transfers.to_string(), actual.to_string());
    }
}
