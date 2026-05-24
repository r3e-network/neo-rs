use crate::hardfork::Hardfork;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

pub(super) fn token_management_methods() -> Vec<NativeMethod> {
    vec![
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
            ContractParameterType::InteropInterface,
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
                ContractParameterType::Hash160,
            ],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfFaun)
        .with_parameter_names(vec!["assetId".to_string(), "account".to_string()]),
        NativeMethod::new(
            "mint".to_string(),
            1 << 15,
            false,
            CallFlags::WRITE_STATES.bits() | CallFlags::ALLOW_CALL.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfFaun)
        .with_parameter_names(vec![
            "assetId".to_string(),
            "account".to_string(),
            "amount".to_string(),
        ]),
        NativeMethod::new(
            "burn".to_string(),
            1 << 15,
            false,
            CallFlags::WRITE_STATES.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
            ],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfFaun)
        .with_parameter_names(vec!["assetId".to_string(), "account".to_string()]),
        NativeMethod::new(
            "burn".to_string(),
            1 << 15,
            false,
            CallFlags::WRITE_STATES.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfFaun)
        .with_parameter_names(vec![
            "assetId".to_string(),
            "account".to_string(),
            "amount".to_string(),
        ]),
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
    ]
}
