use crate::util::Uint160;
use crate::encoding::address;
use serde::{Serialize, Deserialize};

/// Token represents an imported token contract.
#[derive(Serialize, Deserialize)]
pub struct Token {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "script_hash")]
    pub hash: Uint160,
    #[serde(rename = "decimals")]
    pub decimals: i64,
    #[serde(rename = "symbol")]
    pub symbol: String,
    #[serde(rename = "standard")]
    pub standard: String,
}

impl Token {
    /// NewToken returns the new token contract info.
    pub fn new(token_hash: Uint160, name: String, symbol: String, decimals: i64, standard_name: String) -> Self {
        Token {
            name,
            hash: token_hash,
            decimals,
            symbol,
            standard: standard_name,
        }
    }

    /// Address returns token address from hash.
    pub fn address(&self) -> String {
        address::uint160_to_string(&self.hash)
    }
}
