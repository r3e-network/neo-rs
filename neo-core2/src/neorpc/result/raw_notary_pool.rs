use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::str::FromStr;

use crate::util::Uint256;

// RawNotaryPool represents a result of `getrawnotarypool` RPC call.
// The structure consists of `Hashes`. `Hashes` field is a map, where key is
// the hash of the main transaction and value is a slice of related fallback
// transaction hashes.
#[derive(Serialize, Deserialize)]
pub struct RawNotaryPool {
    pub hashes: HashMap<Uint256, Vec<Uint256>>,
}

// rawNotaryPoolAux is an auxiliary struct for RawNotaryPool JSON marshalling.
#[derive(Serialize, Deserialize)]
struct RawNotaryPoolAux {
    hashes: HashMap<String, Vec<Uint256>>,
}

// Implement custom serialization for RawNotaryPool
impl RawNotaryPool {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let mut aux = RawNotaryPoolAux {
            hashes: HashMap::with_capacity(self.hashes.len()),
        };
        for (main, fallbacks) in &self.hashes {
            aux.hashes.insert(format!("0x{}", main.to_string_le()), fallbacks.clone());
        }
        serde_json::to_string(&aux)
    }

    pub fn from_json(data: &str) -> Result<Self, serde_json::Error> {
        let aux: RawNotaryPoolAux = serde_json::from_str(data)?;
        let mut hashes = HashMap::with_capacity(aux.hashes.len());
        for (main, fallbacks) in aux.hashes {
            let hash_main = Uint256::from_str(&main.trim_start_matches("0x")).map_err(|e| serde_json::Error::custom(e.to_string()))?;
            hashes.insert(hash_main, fallbacks);
        }
        Ok(RawNotaryPool { hashes })
    }
}
