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
#[path = "../../../tests/client/models/wallet/rpc_transfer_out.rs"]
mod tests;
