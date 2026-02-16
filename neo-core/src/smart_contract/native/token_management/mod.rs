//! TokenManagement native contract implementation.
//!
//! This module provides the TokenManagement native contract which manages
//! token metadata and operations on the Neo blockchain.

mod methods;
mod types;

use crate::UInt160;
use crate::error::CoreResult;
use crate::hardfork::Hardfork;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::native::NativeContract;
use crate::smart_contract::native::NativeMethod;
use std::any::Any;

pub use types::{NFTState, TokenState, TokenType};

const ID: i32 = -12;
const PREFIX_TOKEN_STATE: u8 = 10;
const PREFIX_ACCOUNT_STATE: u8 = 12;

const NFT_INDEX_KEY_SIZE: usize = 1 + 20 + 20;

const PREFIX_NFT_UNIQUE_ID_SEED: u8 = 15;
const PREFIX_NFT_STATE: u8 = 8;
const PREFIX_NFT_OWNER_UNIQUE_ID_INDEX: u8 = 21;
const PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX: u8 = 23;

pub type AccountState = crate::smart_contract::native::account_state::AccountState;

#[derive(Debug, Clone)]
pub struct TokenManagement {
    methods: Vec<NativeMethod>,
}

impl TokenManagement {
    pub fn new() -> Self {
        let methods = vec![
            NativeMethod::new(
                "getTokenInfo".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Array,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string()]),
            NativeMethod::new(
                "balanceOf".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                ],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string(), "account".to_string()]),
            NativeMethod::new(
                "getAssetsOfOwner".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Array,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["owner".to_string()]),
            NativeMethod::new(
                "create".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::Hash160,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "type".to_string(),
                "owner".to_string(),
                "name".to_string(),
                "symbol".to_string(),
                "decimals".to_string(),
                "maxSupply".to_string(),
                "mintable".to_string(),
            ]),
            NativeMethod::new(
                "createNonFungible".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::String,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::Hash160,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "owner".to_string(),
                "name".to_string(),
                "symbol".to_string(),
                "mintable".to_string(),
            ]),
            NativeMethod::new(
                "mint".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string(), "amountOrNftId".to_string()]),
            NativeMethod::new(
                "burn".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string(), "amountOrNftId".to_string()]),
            NativeMethod::new(
                "transfer".to_string(),
                1 << 15,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "assetId".to_string(),
                "from".to_string(),
                "to".to_string(),
                "amountOrNftId".to_string(),
                "data".to_string(),
            ]),
            NativeMethod::new(
                "mintNFT".to_string(),
                1 << 17,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                ],
                ContractParameterType::Hash160,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string(), "account".to_string()]),
            NativeMethod::new(
                "burnNFT".to_string(),
                1 << 17,
                false,
                CallFlags::WRITE_STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["nftId".to_string()]),
            NativeMethod::new(
                "transferNFT".to_string(),
                1 << 17,
                false,
                CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec![
                "nftId".to_string(),
                "from".to_string(),
                "to".to_string(),
                "data".to_string(),
            ]),
            NativeMethod::new(
                "getNFTInfo".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Array,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["nftId".to_string()]),
            NativeMethod::new(
                "getNFTs".to_string(),
                1 << 22,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::InteropInterface,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["assetId".to_string()]),
            NativeMethod::new(
                "getNFTsOfOwner".to_string(),
                1 << 22,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::InteropInterface,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(vec!["account".to_string()]),
        ];
        Self { methods }
    }
}

impl Default for TokenManagement {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeContract for TokenManagement {
    fn id(&self) -> i32 {
        ID
    }

    fn hash(&self) -> UInt160 {
        UInt160::from([
            0xae, 0x00, 0xc5, 0x7d, 0xae, 0xb2, 0x0f, 0x9b, 0x65, 0x4f, 0x32, 0x65, 0xa9, 0x18,
            0xf4, 0x4a, 0x8a, 0x40, 0xe0, 0x49,
        ])
    }

    fn name(&self) -> &str {
        "TokenManagement"
    }

    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfFaun)
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        Vec::new()
    }

    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfFaun]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    #[test]
    fn test_token_state_default() {
        let state = TokenState::default();
        assert_eq!(state.token_type, TokenType::Fungible);
        assert_eq!(state.total_supply, BigInt::from(0));
    }

    #[test]
    fn test_account_state_new() {
        let state = AccountState::new();
        assert_eq!(state.balance, BigInt::from(0));
    }

    #[test]
    fn test_nft_state_new() {
        let nft = NFTState::new();
        assert!(nft.properties.is_empty());
    }
}
