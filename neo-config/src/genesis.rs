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
                    public_key:
                        "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c"
                            .to_string(),
                    name: Some("Neo Foundation 1".to_string()),
                },
                GenesisValidator {
                    public_key:
                        "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093"
                            .to_string(),
                    name: Some("Neo Foundation 2".to_string()),
                },
                GenesisValidator {
                    public_key:
                        "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a"
                            .to_string(),
                    name: Some("Neo Foundation 3".to_string()),
                },
                GenesisValidator {
                    public_key:
                        "02ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554"
                            .to_string(),
                    name: Some("Neo Foundation 4".to_string()),
                },
                GenesisValidator {
                    public_key:
                        "024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d"
                            .to_string(),
                    name: Some("Neo Foundation 5".to_string()),
                },
                GenesisValidator {
                    public_key:
                        "02aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e"
                            .to_string(),
                    name: Some("Neo Foundation 6".to_string()),
                },
                GenesisValidator {
                    public_key:
                        "02486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70"
                            .to_string(),
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
                "023a36c72844610b4d34d1968662424011bf783ca9d984efa19a20babf5582f3fe".to_string(),
                "03708b860c1de5d87f5b151a12c2a99feebd2e8b315ee8e7cf8aa19692a9e18379".to_string(),
                "03c6aa6e12638b36e88adc1ccdceac4db9929575c3e03576c617c49cce7114a050".to_string(),
                "03204223f8c86b8cd5c89ef12e4f0dbb314172e9241e30c9ef2293790793537cf0".to_string(),
                "02a62c915cf19c7f19a50ec217e79fac2439bbaad658493de0c7d8ffa92ab0aa62".to_string(),
                "03409f31f0d66bdc2f70a9730b66fe186658f84a8018204db01c106edc36553cd0".to_string(),
                "0288342b141c30dc8ffcde0204929bb46aed5756b41ef4a56778d15ada8f0c6654".to_string(),
                "020f2887f41474cfeb11fd262e982051c1541418137c02a0f4961af911045de639".to_string(),
                "0222038884bbd1d8ff109ed3bdef3542e768eef76c1247aea8bc8171f532928c30".to_string(),
                "03d281b42002647f0113f36c7b8efb30db66078dfaaa9ab3ff76d043a98d512fde".to_string(),
                "02504acbc1f4b3bdad1d86d6e1a08603771db135a73e61c9d565ae06a1938cd2ad".to_string(),
                "0226933336f1b75baa42d42b71d9091508b638046d19abd67f4e119bf64a7cfb4d".to_string(),
                "03cdcea66032b82f5c30450e381e5295cae85c5e6943af716cc6b646352a6067dc".to_string(),
                "02cd5a5547119e24feaa7c2a0f37b8c9366216bab7054de0065c9be42084003c8a".to_string(),
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
                    public_key:
                        "023e9b32ea89b94d066e649b124fd50e396ee91369e8e2a6ae1b11c170d022256d"
                            .to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key:
                        "03009b7540e10f2562e5fd8fac9eaec25166a58b26e412348ff5a86927bfac22a2"
                            .to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key:
                        "02ba2c70f5996f357a43198705859fae2cfea13e1172962800772b3d588a9d4abd"
                            .to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key:
                        "03408dcd416396f64783ac587ea1e1593c57d9fea880c8a6a1920e92a259477806"
                            .to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key:
                        "02a7834be9b32e2981d157cb5bbd3acb42cfd11ea5c3b10224d7a44e98c5910f1b"
                            .to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key:
                        "0214baf0ceea3a66f17e7e1e839ea25fd8bed6cd82e6bb6e68250189065f44ff01"
                            .to_string(),
                    name: None,
                },
                GenesisValidator {
                    public_key:
                        "030205e9cefaea5a1dfc580af20c8d5aa2468bb0148f1a5e4605fc622c80e604ba"
                            .to_string(),
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
                "030205e9cefaea5a1dfc580af20c8d5aa2468bb0148f1a5e4605fc622c80e604ba".to_string(),
                "025831cee3708e87d78211bec0d1bfee9f4c85ae784762f042e7f31c0d40c329b8".to_string(),
                "02cf9dc6e85d581480d91e88e8cbeaa0c153a046e89ded08b4cefd851e1d7325b5".to_string(),
                "03840415b0a0fcf066bcc3dc92d8349ebd33a6ab1402ef649bae00e5d9f5840828".to_string(),
                "026328aae34f149853430f526ecaa9cf9c8d78a4ea82d08bdf63dd03c4d0693be6".to_string(),
                "02c69a8d084ee7319cfecf5161ff257aa2d1f53e79bf6c6f164cff5d94675c38b3".to_string(),
                "0207da870cedb777fceff948641021714ec815110ca111ccc7a54c168e065bda70".to_string(),
                "035056669864feea401d8c31e447fb82dd29f342a9476cfd449584ce2a6165e4d7".to_string(),
                "0370c75c54445565df62cfe2e76fbec4ba00d1298867972213530cae6d418da636".to_string(),
                "03957af9e77282ae3263544b7b2458903624adc3f5dee303957cb6570524a5f254".to_string(),
                "03d84d22b8753cf225d263a3a782a4e16ca72ef323cfde04977c74f14873ab1e4c".to_string(),
                "02147c1b1d5728e1954958daff2f88ee2fa50a06890a8a9db3fa9e972b66ae559f".to_string(),
                "03c609bea5a4825908027e4ab217e7efc06e311f19ecad9d417089f14927a173d5".to_string(),
                "0231edee3978d46c335e851c76059166eb8878516f459e085c0dd092f0f1d51c21".to_string(),
                "03184b018d6b2bc093e535519732b3fd3f7551c8cffaf4621dd5a0b89482ca66c9".to_string(),
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
