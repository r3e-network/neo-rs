// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

pub mod check_sign;
pub mod dbft;
pub mod fixed_bytes;
pub mod genesis;
pub mod h160;
pub mod h256;
pub mod opcode;
pub mod script;
pub mod settings;
pub mod verifying;
mod address;
mod role;
mod account_id;
mod vm_state;
mod script_hash;

use std::{string::String};

pub use check_sign::*;
pub use dbft::*;
pub use fixed_bytes::*;
pub use genesis::*;
pub use h160::*;
pub use h256::*;
use neo_base::encoding::{base58::*, bin::*};
use neo_base::hash::{Ripemd160, Sha256};
pub use opcode::*;
pub use script::*;
use serde::{Deserialize, Serialize};
pub use settings::*;
pub use verifying::*;
pub use address::*;
pub use role::*;
pub use account_id::*;
pub use vm_state::*;
pub use script_hash::*;

pub const SCRIPT_HASH_SIZE: usize = H160_SIZE;
pub const ACCOUNT_SIZE: usize = H160_SIZE;
pub const ADDRESS_NEO3: u8 = 0x35;

/// network(u32) + SHA256
pub const SIGN_DATA_SIZE: usize = 4 + H256_SIZE;

pub type Fee = u64;

pub type Extra = Option<serde_json::Map<String, serde_json::Value>>;

#[cfg(test)]
mod test {
    use neo_base::bytes::ToArray;
    use neo_base::encoding::base64::ToBase64;
    use neo_base::encoding::hex::DecodeHex;
    use crate::address::ToNeo3Address;
    use crate::script_hash::ScriptHash;
    use super::*;

    #[test]
    fn test_script_hash() {
        let script = "61479ab68fd5c2c04b254f382d84ddf2f5c67ced"
            .decode_hex()
            .expect("decode hex should be ok");

        let script = ScriptHash(script.to_array());
        assert_eq!("NUnLWXALK2G6gYa7RadPLRiQYunZHnncxg", script.to_neo3_address().as_str());
        assert_eq!("YUeato/VwsBLJU84LYTd8vXGfO0=", script.to_base64_std());
    }
}

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
