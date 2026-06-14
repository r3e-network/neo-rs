use super::super::utility::{
    NepBalanceFieldRefs, balance_list_to_json, insert_nep_balance_fields, parse_balance_list,
    parse_nep_balance_fields, required_script_hash_or_address,
};
use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_primitives::UInt160;
use neo_serialization::json::{JObject, JToken};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// NEP17 balances for an address matching C# `RpcNep17Balances`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balances {
    /// User script hash
    pub user_script_hash: UInt160,

    /// List of token balances
    pub balances: Vec<RpcNep17Balance>,
}

impl RpcNep17Balances {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        balance_list_to_json(
            &self.balances,
            &self.user_script_hash,
            protocol_settings,
            RpcNep17Balance::to_json,
        )
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let (balances, user_script_hash) = parse_balance_list(json, protocol_settings, |obj| {
            RpcNep17Balance::from_json(obj, protocol_settings)
        })?;

        Ok(Self {
            user_script_hash,
            balances,
        })
    }
}

/// Individual NEP17 balance entry matching C# `RpcNep17Balance`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balance {
    /// Asset hash
    pub asset_hash: UInt160,

    /// Balance amount
    pub amount: BigInt,

    /// Last updated block height
    pub last_updated_block: u32,
}

impl RpcNep17Balance {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "assethash".to_string(),
            JToken::String(self.asset_hash.to_string()),
        );
        insert_nep_balance_fields(
            &mut json,
            NepBalanceFieldRefs {
                amount: &self.amount,
                last_updated_block: self.last_updated_block,
            },
        );
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(
        json: &JObject,
        _protocol_settings: &ProtocolSettings,
    ) -> CoreResult<Self> {
        let asset_hash =
            required_script_hash_or_address(json, "assethash", _protocol_settings, "asset hash")?;
        let fields = parse_nep_balance_fields(json)?;

        Ok(Self {
            asset_hash,
            amount: fields.amount,
            last_updated_block: fields.last_updated_block,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::rpc_case_result;
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_serialization::json::{JArray, JToken};
    use neo_wallets::wallet_helper as WalletHelper;

    #[test]
    fn balance_roundtrip() {
        let entry = RpcNep17Balance {
            asset_hash: UInt160::zero(),
            amount: BigInt::from(42),
            last_updated_block: 10,
        };
        let json = entry.to_json();
        let parsed =
            RpcNep17Balance::from_json(&json, &ProtocolSettings::default_settings()).unwrap();
        assert_eq!(parsed.asset_hash, entry.asset_hash);
        assert_eq!(parsed.amount, entry.amount);
        assert_eq!(parsed.last_updated_block, entry.last_updated_block);
    }

    #[test]
    fn balances_roundtrip() {
        let entry = RpcNep17Balance {
            asset_hash: UInt160::zero(),
            amount: BigInt::from(5),
            last_updated_block: 3,
        };
        let balances = RpcNep17Balances {
            user_script_hash: UInt160::zero(),
            balances: vec![entry.clone()],
        };
        let json = balances.to_json(&ProtocolSettings::default_settings());
        let parsed =
            RpcNep17Balances::from_json(&json, &ProtocolSettings::default_settings()).unwrap();

        assert_eq!(parsed.user_script_hash, balances.user_script_hash);
        assert_eq!(parsed.balances.len(), 1);
        assert_eq!(parsed.balances[0].amount, entry.amount);
    }

    #[test]
    fn balances_array_keeps_lossy_parse_behavior() {
        let settings = ProtocolSettings::default_settings();
        let valid = RpcNep17Balance {
            asset_hash: UInt160::zero(),
            amount: BigInt::from(5),
            last_updated_block: 3,
        }
        .to_json();

        let mut malformed = JObject::new();
        malformed.insert(
            "assethash".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );

        let mut balances = JArray::new();
        balances.add(Some(JToken::Object(valid)));
        balances.add(None);
        balances.add(Some(JToken::String("not an object".to_string())));
        balances.add(Some(JToken::Object(malformed)));

        let mut root = JObject::new();
        root.insert("balance".to_string(), JToken::Array(balances));
        root.insert(
            "address".to_string(),
            JToken::String(WalletHelper::to_address(
                &UInt160::zero(),
                settings.address_version,
            )),
        );

        let parsed = RpcNep17Balances::from_json(&root, &settings).unwrap();
        assert_eq!(parsed.balances.len(), 1);
        assert_eq!(parsed.balances[0].amount, BigInt::from(5));
    }

    #[test]
    fn nep17_balances_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("getnep17balancesasync") else {
            return;
        };
        let settings = ProtocolSettings::default_settings();
        let parsed = RpcNep17Balances::from_json(&expected, &settings).expect("parse");
        let actual = parsed.to_json(&settings);
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
