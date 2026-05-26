use super::TokenManagement;
use crate::smart_contract::native::method_macros::neo_native_contract_methods;

macro_rules! token_management_method_table {
    ([$($callback:tt)+]; $($args:tt)*) => {
        $($callback)+! {
            $($args)*
            ;
            {
                safe "getTokenInfo", fee = 1 << 15, flags = [READ_STATES], params = [Hash160], returns = Array, active = HfFaun, names = ["assetId"] => engine invoke_get_token_info;
                safe "balanceOf", fee = 1 << 15, flags = [READ_STATES], params = [Hash160, Hash160], returns = Integer, active = HfFaun, names = ["assetId", "account"] => engine invoke_balance_of;
                safe "getAssetsOfOwner", fee = 1 << 15, flags = [READ_STATES], params = [Hash160], returns = InteropInterface, active = HfFaun, names = ["owner"] => engine invoke_get_assets_of_owner;
                unsafe "create", fee = 1 << 15, flags = [WRITE_STATES, ALLOW_CALL], params = [Integer, Hash160, String, String, Integer, Integer, Boolean], returns = Hash160, active = HfFaun, names = ["type", "owner", "name", "symbol", "decimals", "maxSupply", "mintable"] => engine invoke_create;
                unsafe "createNonFungible", fee = 1 << 15, flags = [WRITE_STATES, ALLOW_CALL], params = [Hash160, String, String, Boolean], returns = Hash160, active = HfFaun, names = ["owner", "name", "symbol", "mintable"] => engine invoke_create_non_fungible;
                unsafe "mint", fee = 1 << 15, flags = [WRITE_STATES, ALLOW_CALL], params = [Hash160, Hash160], returns = Boolean, active = HfFaun, names = ["assetId", "account"] => engine invoke_mint;
                unsafe "mint", fee = 1 << 15, flags = [WRITE_STATES, ALLOW_CALL], params = [Hash160, Hash160, Integer], returns = Boolean, active = HfFaun, names = ["assetId", "account", "amount"] => engine invoke_mint;
                unsafe "burn", fee = 1 << 15, flags = [WRITE_STATES], params = [Hash160, Hash160], returns = Boolean, active = HfFaun, names = ["assetId", "account"] => engine invoke_burn;
                unsafe "burn", fee = 1 << 15, flags = [WRITE_STATES], params = [Hash160, Hash160, Integer], returns = Boolean, active = HfFaun, names = ["assetId", "account", "amount"] => engine invoke_burn;
                unsafe "transfer", fee = 1 << 15, flags = [WRITE_STATES, ALLOW_CALL], params = [Hash160, Hash160, Hash160, Integer, Any], returns = Boolean, active = HfFaun, names = ["assetId", "from", "to", "amountOrNftId", "data"] => engine invoke_transfer;
                unsafe "mintNFT", fee = 1 << 17, flags = [WRITE_STATES, ALLOW_CALL], params = [Hash160, Hash160], returns = Hash160, active = HfFaun, names = ["assetId", "account"] => engine invoke_mint_nft;
                unsafe "burnNFT", fee = 1 << 17, flags = [WRITE_STATES], params = [Hash160], returns = Boolean, active = HfFaun, names = ["nftId"] => engine invoke_burn_nft;
                unsafe "transferNFT", fee = 1 << 17, flags = [WRITE_STATES, ALLOW_CALL], params = [Hash160, Hash160, Hash160, Any], returns = Boolean, active = HfFaun, names = ["nftId", "from", "to", "data"] => engine invoke_transfer_nft;
                safe "getNFTInfo", fee = 1 << 15, flags = [READ_STATES], params = [Hash160], returns = Array, active = HfFaun, names = ["nftId"] => engine invoke_get_nft_info;
                safe "getNFTs", fee = 1 << 22, flags = [READ_STATES], params = [Hash160], returns = InteropInterface, active = HfFaun, names = ["assetId"] => engine invoke_get_nfts;
                safe "getNFTsOfOwner", fee = 1 << 22, flags = [READ_STATES], params = [Hash160], returns = InteropInterface, active = HfFaun, names = ["account"] => engine invoke_get_nfts_of_owner;
            }
        }
    };

    ($callback:ident; $($args:tt)*) => {
        token_management_method_table!([$callback]; $($args)*)
    };
}

neo_native_contract_methods!(
    TokenManagement,
    table = token_management_method_table,
    aliases = [],
    unknown = |method| crate::error::CoreError::native_contract(format!(
        "TokenManagement: unknown method '{}'",
        method
    ))
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_management_method_metadata_snapshot() {
        let snapshot = TokenManagement::native_methods()
            .iter()
            .map(|method| {
                format!(
                    "{}|{}|{}|{}|{}|{:?}|{:?}|{}|{:?}|{:?}",
                    method.name,
                    method.cpu_fee,
                    method.storage_fee,
                    method.safe,
                    method.required_call_flags,
                    method.parameters,
                    method.return_type,
                    method.parameter_names.join(","),
                    method.active_in,
                    method.deprecated_in
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(
            snapshot,
            "\
getTokenInfo|32768|0|true|1|[Hash160]|Array|assetId|Some(HfFaun)|None
balanceOf|32768|0|true|1|[Hash160, Hash160]|Integer|assetId,account|Some(HfFaun)|None
getAssetsOfOwner|32768|0|true|1|[Hash160]|InteropInterface|owner|Some(HfFaun)|None
create|32768|0|false|6|[Integer, Hash160, String, String, Integer, Integer, Boolean]|Hash160|type,owner,name,symbol,decimals,maxSupply,mintable|Some(HfFaun)|None
createNonFungible|32768|0|false|6|[Hash160, String, String, Boolean]|Hash160|owner,name,symbol,mintable|Some(HfFaun)|None
mint|32768|0|false|6|[Hash160, Hash160]|Boolean|assetId,account|Some(HfFaun)|None
mint|32768|0|false|6|[Hash160, Hash160, Integer]|Boolean|assetId,account,amount|Some(HfFaun)|None
burn|32768|0|false|2|[Hash160, Hash160]|Boolean|assetId,account|Some(HfFaun)|None
burn|32768|0|false|2|[Hash160, Hash160, Integer]|Boolean|assetId,account,amount|Some(HfFaun)|None
transfer|32768|0|false|6|[Hash160, Hash160, Hash160, Integer, Any]|Boolean|assetId,from,to,amountOrNftId,data|Some(HfFaun)|None
mintNFT|131072|0|false|6|[Hash160, Hash160]|Hash160|assetId,account|Some(HfFaun)|None
burnNFT|131072|0|false|2|[Hash160]|Boolean|nftId|Some(HfFaun)|None
transferNFT|131072|0|false|6|[Hash160, Hash160, Hash160, Any]|Boolean|nftId,from,to,data|Some(HfFaun)|None
getNFTInfo|32768|0|true|1|[Hash160]|Array|nftId|Some(HfFaun)|None
getNFTs|4194304|0|true|1|[Hash160]|InteropInterface|assetId|Some(HfFaun)|None
getNFTsOfOwner|4194304|0|true|1|[Hash160]|InteropInterface|account|Some(HfFaun)|None"
        );
    }
}
