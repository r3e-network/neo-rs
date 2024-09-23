use serde::{Deserialize, Serialize};
use serde_json;
use std::error::Error;
use std::fmt;
use std::str::FromStr;
use num_bigint::BigInt;
use crate::encoding::address;
use crate::util::Uint160;

// UnclaimedGas response wrapper.
#[derive(Debug, Serialize, Deserialize)]
pub struct UnclaimedGas {
    pub address: Uint160,
    pub unclaimed: BigInt,
}

// unclaimedGas is an auxiliary struct for JSON marshalling.
#[derive(Debug, Serialize, Deserialize)]
struct UnclaimedGasAux {
    address: String,
    unclaimed: String,
}

impl UnclaimedGas {
    // MarshalJSON implements the json.Marshaler interface.
    pub fn marshal_json(&self) -> Result<String, Box<dyn Error>> {
        let gas_aux = UnclaimedGasAux {
            address: address::uint160_to_string(&self.address),
            unclaimed: self.unclaimed.to_string(),
        };
        let json = serde_json::to_string(&gas_aux)?;
        Ok(json)
    }

    // UnmarshalJSON implements the json.Unmarshaler interface.
    pub fn unmarshal_json(data: &str) -> Result<UnclaimedGas, Box<dyn Error>> {
        let gas_aux: UnclaimedGasAux = serde_json::from_str(data)?;
        let unclaimed = BigInt::from_str(&gas_aux.unclaimed)
            .map_err(|_| "failed to convert unclaimed gas")?;
        let address = address::string_to_uint160(&gas_aux.address)?;
        Ok(UnclaimedGas {
            address,
            unclaimed,
        })
    }
}

impl fmt::Display for UnclaimedGas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address: {}, Unclaimed: {}", self.address, self.unclaimed)
    }
}
