//! Genesis block configuration

use serde::{Deserialize, Serialize};

/// Genesis block configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Genesis block timestamp (Unix timestamp in milliseconds)
    pub timestamp: u64,

    /// Initial validators for consensus
    pub validators: Vec<GenesisValidator>,

    /// Committee members (superset of validators)
    pub committee: Vec<String>,

    /// Initial token distribution
    pub distribution: Vec<TokenDistribution>,

    /// Initial contract deployments
    #[serde(default)]
    pub contracts: Vec<GenesisContract>,
}

/// Genesis validator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidator {
    /// Validator public key (hex encoded, compressed ECPoint)
    pub public_key: String,

    /// Validator name (optional, for display)
    #[serde(default)]
    pub name: Option<String>,
}

/// Initial token distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenDistribution {
    /// Token type (NEO or GAS)
    pub token: TokenType,

    /// Recipient address (Neo address format)
    pub address: String,

    /// Amount to distribute
    pub amount: u64,
}

/// Token types for genesis distribution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TokenType {
    /// NEO governance token
    Neo,
    /// GAS utility token
    Gas,
}

/// Genesis contract deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisContract {
    /// Contract name
    pub name: String,

    /// Contract hash (script hash)
    pub hash: String,

    /// Contract script (base64 encoded)
    pub script: String,

    /// Contract manifest (JSON)
    pub manifest: String,
}

impl Default for GenesisConfig {
    fn default() -> Self {
        Self::mainnet()
    }
}

impl GenesisConfig {
    /// MainNet genesis configuration
    pub fn mainnet() -> Self {
        Self {
            // Neo N3 MainNet genesis timestamp: 2021-03-20T15:00:00Z
            timestamp: 1616245200000,
            validators: vec![
                GenesisValidator {
                    public_key: "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c".to_string(),
                    name: Some("Neo Foundation 1".to_string()),
                },
                GenesisValidator {
                    public_key: "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093".to_string(),
                    name: Some("Neo Foundation 2".to_string()),
                },
                GenesisValidator {
                    public_key: "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a".to_string(),
                    name: Some("Neo Foundation 3".to_string()),
                },
                GenesisValidator {
                    public_key: "02ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554".to_string(),
                    name: Some("Neo Foundation 4".to_string()),
                },
                GenesisValidator {
                    public_key: "024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d".to_string(),
                    name: Some("Neo Foundation 5".to_string()),
                },
                GenesisValidator {
                    public_key: "02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e".to_string(),
                    name: Some("Neo Foundation 6".to_string()),
                },
                GenesisValidator {
                    public_key: "02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70".to_string(),
                    name: Some("Neo Foundation 7".to_string()),
                },
            ],
            committee: vec![
                "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c".to_string(),
                "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093".to_string(),
                "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a".to_string(),
                "02ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554".to_string(),
                "024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d".to_string(),
                "02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e".to_string(),
                "02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70".to_string(),
            ],
            distribution: vec![],
            contracts: vec![],
        }
    }

    /// TestNet genesis configuration
    pub fn testnet() -> Self {
        Self {
            timestamp: 1616245200000,
            validators: vec![
                GenesisValidator {
                    public_key: "023e9b32ea89b94d066e649b124fd50e396ee91369e8e2a6ae1b11c170d022256d".to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key: "03009b7540e10f2562e5fd8fac9eaec25166a58b26e412348ff5a86927bfac22a2".to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key: "02ba2c70f5996f357a43198705859fae2cfea13e1172962800772b3d588a9d4abd".to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key: "03408dcd416396f64783ac587ea1e1593c57d9fea880c8a6a1920e92a259477806".to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key: "02a7834be9b32e2981d157cb5bbd3acb42cfd11ea5c3b10224d7a44e98c5910f1b".to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key: "0214baf0ceea3a66f17e7e1e839ea25fd8bed6cd82e6bb6e68250189065f44ff01".to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key: "030205e9cefaea5a1dfc580571e0b0123f3b4e55e1ceda5e6a0c7ecab2e01e7e01".to_string(),
                    name: None,
                },
            ],
            committee: vec![
                "023e9b32ea89b94d066e649b124fd50e396ee91369e8e2a6ae1b11c170d022256d".to_string(),
                "03009b7540e10f2562e5fd8fac9eaec25166a58b26e412348ff5a86927bfac22a2".to_string(),
                "02ba2c70f5996f357a43198705859fae2cfea13e1172962800772b3d588a9d4abd".to_string(),
                "03408dcd416396f64783ac587ea1e1593c57d9fea880c8a6a1920e92a259477806".to_string(),
                "02a7834be9b32e2981d157cb5bbd3acb42cfd11ea5c3b10224d7a44e98c5910f1b".to_string(),
                "0214baf0ceea3a66f17e7e1e839ea25fd8bed6cd82e6bb6e68250189065f44ff01".to_string(),
                "030205e9cefaea5a1dfc580571e0b0123f3b4e55e1ceda5e6a0c7ecab2e01e7e01".to_string(),
            ],
            distribution: vec![],
            contracts: vec![],
        }
    }

    /// Create a private network genesis with single validator
    pub fn private(validator_pubkey: &str) -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            validators: vec![GenesisValidator {
                public_key: validator_pubkey.to_string(),
                name: Some("Local Validator".to_string()),
            }],
            committee: vec![validator_pubkey.to_string()],
            distribution: vec![],
            contracts: vec![],
        }
    }

    /// Validate the genesis configuration
    pub fn validate(&self) -> crate::ConfigResult<()> {
        if self.validators.is_empty() {
            return Err(crate::ConfigError::GenesisError(
                "At least one validator is required".to_string(),
            ));
        }

        for validator in &self.validators {
            if validator.public_key.len() != 66 {
                return Err(crate::ConfigError::GenesisError(format!(
                    "Invalid validator public key length: {}",
                    validator.public_key
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_genesis() {
        let genesis = GenesisConfig::mainnet();
        assert_eq!(genesis.validators.len(), 7);
        assert!(genesis.validate().is_ok());
    }

    #[test]
    fn test_testnet_genesis() {
        let genesis = GenesisConfig::testnet();
        assert_eq!(genesis.validators.len(), 7);
        assert!(genesis.validate().is_ok());
    }

    #[test]
    fn test_private_genesis() {
        let genesis = GenesisConfig::private(
            "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
        );
        assert_eq!(genesis.validators.len(), 1);
        assert!(genesis.validate().is_ok());
    }

    #[test]
    fn test_invalid_genesis() {
        let genesis = GenesisConfig {
            timestamp: 0,
            validators: vec![],
            committee: vec![],
            distribution: vec![],
            contracts: vec![],
        };
        assert!(genesis.validate().is_err());
    }
}
