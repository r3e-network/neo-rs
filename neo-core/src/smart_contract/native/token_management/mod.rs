//! TokenManagement native contract implementation.
//!
//! This module provides the TokenManagement native contract which manages
//! token metadata and operations on the Neo blockchain.

use crate::error::CoreError;
use crate::error::CoreResult;
use crate::hardfork::Hardfork;
use crate::persistence::seek_direction::SeekDirection;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::iterators::StorageIterator;
use crate::smart_contract::native::NativeContract;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::StorageItem;
use crate::smart_contract::StorageKey;
use crate::UInt160;
use num_bigint::BigInt;
use num_traits::Signed;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::any::Any;

mod events;
mod ids;
mod indexes;
mod methods;
mod stack_value;
mod state;
mod storage;
pub use state::{AccountState, NFTState, TokenState, TokenType};

const ID: i32 = -12;
const PREFIX_TOKEN_STATE: u8 = 10;
const PREFIX_ACCOUNT_STATE: u8 = 12;

const NFT_INDEX_KEY_SIZE: usize = 1 + 20 + 20;

const PREFIX_NFT_UNIQUE_ID_SEED: u8 = 15;
const PREFIX_NFT_STATE: u8 = 8;
const PREFIX_NFT_OWNER_UNIQUE_ID_INDEX: u8 = 21;
const PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX: u8 = 23;

#[derive(Debug, Clone)]
pub struct TokenManagement {
    methods: Vec<NativeMethod>,
}

impl TokenManagement {
    pub fn new() -> Self {
        Self {
            methods: methods::token_management_methods(),
        }
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

impl TokenManagement {
    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "getTokenInfo" => self.invoke_get_token_info(engine, args),
            "balanceOf" => self.invoke_balance_of(engine, args),
            "getAssetsOfOwner" => self.invoke_get_assets_of_owner(engine, args),
            "create" => self.invoke_create(engine, args),
            "createNonFungible" => self.invoke_create_non_fungible(engine, args),
            "mint" => self.invoke_mint(engine, args),
            "burn" => self.invoke_burn(engine, args),
            "transfer" => self.invoke_transfer(engine, args),
            "mintNFT" => self.invoke_mint_nft(engine, args),
            "burnNFT" => self.invoke_burn_nft(engine, args),
            "transferNFT" => self.invoke_transfer_nft(engine, args),
            "getNFTInfo" => self.invoke_get_nft_info(engine, args),
            "getNFTs" => self.invoke_get_nfts(engine, args),
            "getNFTsOfOwner" => self.invoke_get_nfts_of_owner(engine, args),
            _ => Err(CoreError::native_contract(format!(
                "TokenManagement: unknown method '{}'",
                method
            ))),
        }
    }

    fn invoke_get_token_info(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "TokenManagement.getTokenInfo: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;

        let Some(token_state) = self.get_token_state(engine, &asset_id)? else {
            return Err(CoreError::native_contract(
                "TokenManagement.getTokenInfo: token not found",
            ));
        };

        let bytes = Self::serialize_storage_stack_value(&token_state.to_stack_value())
            .map_err(CoreError::native_contract)?;
        Ok(bytes)
    }

    fn invoke_balance_of(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.balanceOf: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let account = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let Some(account_state) = self.get_account_state(engine, &asset_id, &account)? else {
            return Ok(vec![0]);
        };

        let bytes = account_state.balance.to_signed_bytes_le();
        Ok(bytes)
    }

    fn invoke_get_assets_of_owner(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "TokenManagement.getAssetsOfOwner: invalid arguments",
            ));
        }

        let account = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let prefix = StorageKey::create(ID, PREFIX_ACCOUNT_STATE);
        let entries: Vec<(StorageKey, StorageItem)> = engine
            .snapshot_cache()
            .find(Some(&prefix), SeekDirection::Forward)
            .collect();

        let filtered: Vec<(StorageKey, StorageItem)> = entries
            .into_iter()
            .filter(|(key, _)| {
                let suffix = key.suffix();
                if suffix.len() < 1 + 20 + 20 {
                    return false;
                }
                let account_from_key = UInt160::from_bytes(&suffix[1..21]).ok();
                account_from_key == Some(account)
            })
            .collect();

        let options = FindOptions::RemovePrefix | FindOptions::DeserializeValues;
        let iterator = StorageIterator::new(filtered, 1, options);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(CoreError::native_contract)?;

        Ok(iterator_id.to_le_bytes().to_vec())
    }

    fn invoke_create(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 7 {
            return Err(CoreError::native_contract(
                "TokenManagement.create: invalid arguments",
            ));
        }

        let token_type = if args[0].is_empty() || args[0][0] == 0 {
            TokenType::Fungible
        } else {
            TokenType::NonFungible
        };

        let owner = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid owner"))?;

        let name = String::from_utf8_lossy(&args[2]).to_string();
        let symbol = String::from_utf8_lossy(&args[3]).to_string();

        let decimals = if args[4].is_empty() {
            0
        } else {
            BigInt::from_signed_bytes_le(&args[4])
                .to_u8()
                .ok_or_else(|| CoreError::native_contract("Invalid decimals"))?
        };

        let max_supply = BigInt::from_signed_bytes_le(&args[5]);
        let mintable = !args[6].is_empty() && args[6][0] != 0;

        let asset_id = TokenManagement::get_asset_id(&owner, &name);

        let context = engine.get_native_storage_context(&self.hash())?;

        if self.get_token_state(engine, &asset_id)?.is_some() {
            return Err(CoreError::native_contract(
                "TokenManagement.create: token already exists",
            ));
        }

        let mintable_address = if mintable { Some(owner) } else { None };

        let token_state = TokenState {
            token_type,
            owner,
            name,
            symbol,
            decimals,
            total_supply: BigInt::zero(),
            max_supply,
            mintable_address,
        };

        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();
        let bytes = Self::serialize_storage_stack_value(&token_state.to_stack_value())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &key, &bytes)?;

        self.emit_created_event(engine, &asset_id, &token_type)?;

        Ok(asset_id.to_bytes().to_vec())
    }

    fn invoke_create_non_fungible(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 4 {
            return Err(CoreError::native_contract(
                "TokenManagement.createNonFungible: invalid arguments",
            ));
        }

        let owner = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid owner"))?;

        let name = String::from_utf8_lossy(&args[1]).to_string();
        let symbol = String::from_utf8_lossy(&args[2]).to_string();

        let mintable = !args[3].is_empty() && args[3][0] != 0;

        let asset_id = TokenManagement::get_asset_id(&owner, &name);

        let context = engine.get_native_storage_context(&self.hash())?;

        if self.get_token_state(engine, &asset_id)?.is_some() {
            return Err(CoreError::native_contract(
                "TokenManagement.createNonFungible: token already exists",
            ));
        }

        let mintable_address = if mintable { Some(owner) } else { None };

        let token_state = TokenState {
            token_type: TokenType::NonFungible,
            owner,
            name,
            symbol,
            decimals: 0,
            total_supply: BigInt::zero(),
            max_supply: BigInt::zero(),
            mintable_address,
        };

        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();
        let bytes = Self::serialize_storage_stack_value(&token_state.to_stack_value())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &key, &bytes)?;

        self.emit_created_event(engine, &asset_id, &TokenType::NonFungible)?;

        Ok(asset_id.to_bytes().to_vec())
    }

    fn invoke_mint(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if !(2..=3).contains(&args.len()) {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let account = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let amount = if args.len() > 2 {
            BigInt::from_signed_bytes_le(&args[2])
        } else {
            BigInt::from(1)
        };

        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: amount cannot be negative",
            ));
        }

        let context = engine.get_native_storage_context(&self.hash())?;

        let Some(mut token_state) = self.get_token_state(engine, &asset_id)? else {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: token not found",
            ));
        };

        if token_state.max_supply > BigInt::zero()
            && token_state.total_supply.clone() + &amount > token_state.max_supply
        {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: max supply exceeded",
            ));
        }

        if let Some(ref mintable_address) = token_state.mintable_address {
            let caller = engine.calling_script_hash();
            if caller != *mintable_address && !engine.check_witness_hash(mintable_address)? {
                return Ok(vec![0]);
            }
        } else {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: token is not mintable",
            ));
        }

        let mut account_state = self
            .get_account_state(engine, &asset_id, &account)?
            .unwrap_or_default();

        account_state.balance += &amount;
        token_state.total_supply += &amount;

        self.write_account_state(&context, engine, &account, &asset_id, &account_state)?;

        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();
        let bytes = Self::serialize_storage_stack_value(&token_state.to_stack_value())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &key, &bytes)?;

        self.emit_transfer_event(engine, None, Some(&account), &amount)?;

        Ok(vec![1])
    }

    fn invoke_burn(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if !(2..=3).contains(&args.len()) {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let account = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let amount = if args.len() > 2 {
            BigInt::from_signed_bytes_le(&args[2])
        } else {
            BigInt::from(1)
        };

        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: amount cannot be negative",
            ));
        }

        let caller = engine.calling_script_hash();
        if caller != account && !engine.check_witness_hash(&account)? {
            return Ok(vec![0]);
        }

        let context = engine.get_native_storage_context(&self.hash())?;

        let Some(mut token_state) = self.get_token_state(engine, &asset_id)? else {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: token not found",
            ));
        };

        let Some(mut account_state) = self.get_account_state(engine, &asset_id, &account)? else {
            return Ok(vec![0]);
        };

        if account_state.balance < amount {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: insufficient balance",
            ));
        }

        account_state.balance -= &amount;
        token_state.total_supply -= &amount;

        if account_state.balance.is_zero() {
            let asset_key = [
                vec![PREFIX_ACCOUNT_STATE],
                account.to_bytes().to_vec(),
                asset_id.to_bytes().to_vec(),
            ]
            .concat();
            engine.delete_storage_item(&context, &asset_key)?;
        } else {
            self.write_account_state(&context, engine, &account, &asset_id, &account_state)?;
        }

        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();
        let bytes = Self::serialize_storage_stack_value(&token_state.to_stack_value())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &key, &bytes)?;

        self.emit_transfer_event(engine, Some(&account), None, &amount)?;

        Ok(vec![1])
    }

    fn invoke_mint_nft(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.mintNFT: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let account = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let token_key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &asset_id)
            .suffix()
            .to_vec();

        let token_data = match engine.get_storage_item(&context, &token_key) {
            Some(data) => data,
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.mintNFT: asset not found",
                ));
            }
        };

        let token_state = match Self::deserialize_token_state(&token_data) {
            Some(state) => state,
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.mintNFT: invalid token state",
                ));
            }
        };

        if token_state.token_type != TokenType::NonFungible {
            return Err(CoreError::native_contract(
                "TokenManagement.mintNFT: asset is not NFT",
            ));
        }

        let calling_hash = engine.calling_script_hash();
        if token_state.owner != calling_hash && !calling_hash.is_zero() {
            return Err(CoreError::native_contract(format!(
                "TokenManagement.mintNFT: only owner can mint (owner={}, calling={})",
                token_state.owner.to_hex_string(),
                calling_hash.to_hex_string()
            )));
        }

        let unique_id = self.get_next_nft_unique_id(engine)?;

        let new_supply = token_state.total_supply.clone() + 1;
        let mut updated_token_state = token_state.clone();
        updated_token_state.total_supply = new_supply;

        let token_bytes =
            Self::serialize_storage_stack_value(&updated_token_state.to_stack_value())
                .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &token_key, &token_bytes)?;

        let nft_state = NFTState {
            asset_id,
            owner: account,
            properties: Vec::new(),
        };
        let nft_bytes = Self::serialize_storage_stack_value(&nft_state.to_stack_value())
            .map_err(CoreError::native_contract)?;
        let nft_key = StorageKey::create_with_uint160(ID, PREFIX_NFT_STATE, &unique_id)
            .suffix()
            .to_vec();
        engine.put_storage_item(&context, &nft_key, &nft_bytes)?;

        let account_key = [
            vec![PREFIX_ACCOUNT_STATE],
            account.to_bytes().to_vec(),
            asset_id.to_bytes().to_vec(),
        ]
        .concat();
        let mut account_balance = BigInt::from(0);
        if let Some(account_data) = engine.get_storage_item(&context, &account_key) {
            if let Some(state) = Self::deserialize_account_state(&account_data) {
                account_balance = state.balance;
            }
        }
        account_balance += 1;

        let account_state = AccountState::with_balance(account_balance);
        self.write_account_state(&context, engine, &account, &asset_id, &account_state)?;

        self.add_nft_to_asset_index(&context, engine, &asset_id, &unique_id)?;
        self.add_nft_to_owner_index(&context, engine, &account, &unique_id)?;

        self.emit_transfer_event(engine, None, Some(&account), &BigInt::from(1))?;

        Ok(unique_id.to_bytes().to_vec())
    }

    fn invoke_burn_nft(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "TokenManagement.burnNFT: invalid arguments",
            ));
        }

        let nft_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid NFT ID"))?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let nft_key = StorageKey::create_with_uint160(ID, PREFIX_NFT_STATE, &nft_id)
            .suffix()
            .to_vec();

        let nft_data = match engine.get_storage_item(&context, &nft_key) {
            Some(data) => data,
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.burnNFT: NFT not found",
                ));
            }
        };

        let nft_state = match Self::deserialize_nft_state(&nft_data) {
            Some(state) => state,
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.burnNFT: invalid NFT state",
                ));
            }
        };

        if nft_state.owner != engine.calling_script_hash()
            && !engine.calling_script_hash().is_zero()
        {
            return Err(CoreError::native_contract(
                "TokenManagement.burnNFT: only owner can burn",
            ));
        }

        let token_key =
            StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, &nft_state.asset_id)
                .suffix()
                .to_vec();
        let token_data = match engine.get_storage_item(&context, &token_key) {
            Some(data) => data,
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.burnNFT: asset not found",
                ));
            }
        };

        let mut token_state = match Self::deserialize_token_state(&token_data) {
            Some(state) => state,
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.burnNFT: invalid token state",
                ));
            }
        };

        token_state.total_supply -= 1;
        let token_bytes = Self::serialize_storage_stack_value(&token_state.to_stack_value())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &token_key, &token_bytes)?;

        self.update_account_balance(&context, engine, &nft_state.owner, &nft_state.asset_id, -1)?;

        engine.delete_storage_item(&context, &nft_key)?;

        self.remove_nft_from_asset_index(&context, engine, &nft_state.asset_id, &nft_id)?;
        self.remove_nft_from_owner_index(&context, engine, &nft_state.owner, &nft_id)?;

        self.emit_transfer_event(engine, Some(&nft_state.owner), None, &BigInt::from(1))?;

        Ok(vec![1])
    }

    fn invoke_transfer_nft(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 4 {
            return Err(CoreError::native_contract(
                "TokenManagement.transferNFT: invalid arguments",
            ));
        }

        let nft_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid NFT ID"))?;
        let from = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid from"))?;
        let to =
            UInt160::from_bytes(&args[2]).map_err(|_| CoreError::native_contract("Invalid to"))?;

        if from == to {
            return Err(CoreError::native_contract(
                "TokenManagement.transferNFT: cannot transfer to same account",
            ));
        }

        let calling_hash = engine.calling_script_hash();
        if from != calling_hash && !calling_hash.is_zero() && !engine.check_witness(&from)? {
            return Ok(vec![0]);
        }

        let context = engine.get_native_storage_context(&self.hash())?;
        let nft_key = StorageKey::create_with_uint160(ID, PREFIX_NFT_STATE, &nft_id)
            .suffix()
            .to_vec();

        let nft_data = match engine.get_storage_item(&context, &nft_key) {
            Some(data) => data,
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.transferNFT: NFT not found",
                ));
            }
        };

        let mut nft_state = match Self::deserialize_nft_state(&nft_data) {
            Some(state) => state,
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.transferNFT: invalid NFT state",
                ));
            }
        };

        if nft_state.owner != from {
            return Err(CoreError::native_contract(format!(
                "TokenManagement.transferNFT: NFT owner mismatch (owner={}, from={})",
                nft_state.owner.to_hex_string(),
                from.to_hex_string()
            )));
        }

        nft_state.owner = to;
        let nft_bytes = Self::serialize_storage_stack_value(&nft_state.to_stack_value())
            .map_err(CoreError::native_contract)?;
        engine.put_storage_item(&context, &nft_key, &nft_bytes)?;

        self.remove_nft_from_owner_index(&context, engine, &from, &nft_id)?;
        self.add_nft_to_owner_index(&context, engine, &to, &nft_id)?;

        self.update_account_balance(&context, engine, &from, &nft_state.asset_id, -1)?;
        self.update_account_balance(&context, engine, &to, &nft_state.asset_id, 1)?;

        self.emit_transfer_event(engine, Some(&from), Some(&to), &BigInt::from(1))?;

        Ok(vec![1])
    }

    fn invoke_get_nft_info(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "TokenManagement.getNFTInfo: invalid arguments",
            ));
        }

        let nft_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid NFT ID"))?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let nft_key = StorageKey::create_with_uint160(ID, PREFIX_NFT_STATE, &nft_id)
            .suffix()
            .to_vec();

        match engine.get_storage_item(&context, &nft_key) {
            Some(data) => Ok(data),
            None => Ok(vec![]),
        }
    }

    fn invoke_get_nfts(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "TokenManagement.getNFTs: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;

        let snapshot = engine.snapshot_cache();
        let prefix = StorageKey::create(ID, PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX);

        let mut entries_map = std::collections::BTreeMap::new();
        let mut snapshot_keys: std::collections::HashSet<Vec<u8>> =
            std::collections::HashSet::new();

        for (key, value) in snapshot
            .as_ref()
            .find(Some(&prefix), SeekDirection::Forward)
        {
            entries_map.insert(key.clone(), value);
            snapshot_keys.insert(key.suffix().to_vec());
        }

        for (key, _trackable) in snapshot.tracked_items() {
            if key.id != ID {
                continue;
            }
            let suffix = key.suffix();
            if suffix.is_empty() || suffix[0] != PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX {
                continue;
            }
            snapshot_keys.insert(suffix.to_vec());
        }

        for (key, value) in engine
            .original_snapshot_cache()
            .find(Some(&prefix), SeekDirection::Forward)
        {
            if !snapshot_keys.contains(key.suffix()) {
                entries_map.entry(key).or_insert(value);
            }
        }

        let mut entries: Vec<(StorageKey, StorageItem)> = entries_map.into_iter().collect();
        entries.sort_by(|a, b| a.0.suffix().cmp(b.0.suffix()));

        let filtered: Vec<(StorageKey, StorageItem)> = entries
            .into_iter()
            .filter(|(key, _)| {
                let suffix = key.suffix();
                if suffix.len() < 1 + 20 {
                    return false;
                }
                let key_asset_id = UInt160::from_bytes(&suffix[1..21]).ok();
                key_asset_id == Some(asset_id)
            })
            .collect();

        let options = FindOptions::KeysOnly | FindOptions::RemovePrefix;
        let iterator = StorageIterator::new(filtered, 21, options);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(CoreError::native_contract)?;

        Ok(iterator_id.to_le_bytes().to_vec())
    }

    fn invoke_get_nfts_of_owner(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "TokenManagement.getNFTsOfOwner: invalid arguments",
            ));
        }

        let account = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid account"))?;

        let snapshot = engine.snapshot_cache();
        let prefix = StorageKey::create(ID, PREFIX_NFT_OWNER_UNIQUE_ID_INDEX);

        let mut entries_map = std::collections::BTreeMap::new();
        let mut snapshot_keys: std::collections::HashSet<Vec<u8>> =
            std::collections::HashSet::new();

        for (key, value) in snapshot
            .as_ref()
            .find(Some(&prefix), SeekDirection::Forward)
        {
            entries_map.insert(key.clone(), value);
            snapshot_keys.insert(key.suffix().to_vec());
        }

        for (key, _trackable) in snapshot.tracked_items() {
            if key.id != ID {
                continue;
            }
            let suffix = key.suffix();
            if suffix.is_empty() || suffix[0] != PREFIX_NFT_OWNER_UNIQUE_ID_INDEX {
                continue;
            }
            snapshot_keys.insert(suffix.to_vec());
        }

        for (key, value) in engine
            .original_snapshot_cache()
            .find(Some(&prefix), SeekDirection::Forward)
        {
            if !snapshot_keys.contains(key.suffix()) {
                entries_map.entry(key).or_insert(value);
            }
        }

        let mut entries: Vec<(StorageKey, StorageItem)> = entries_map.into_iter().collect();
        entries.sort_by(|a, b| a.0.suffix().cmp(b.0.suffix()));

        let filtered: Vec<(StorageKey, StorageItem)> = entries
            .into_iter()
            .filter(|(key, _)| {
                let suffix = key.suffix();
                if suffix.len() < 1 + 20 {
                    return false;
                }
                let key_account = UInt160::from_bytes(&suffix[1..21]).ok();
                key_account == Some(account)
            })
            .collect();

        let options = FindOptions::KeysOnly | FindOptions::RemovePrefix;
        let iterator = StorageIterator::new(filtered, 21, options);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(CoreError::native_contract)?;

        Ok(iterator_id.to_le_bytes().to_vec())
    }
}

impl TokenManagement {
    fn invoke_transfer(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 5 {
            return Err(CoreError::native_contract(
                "TokenManagement.transfer: invalid arguments",
            ));
        }

        let asset_id = UInt160::from_bytes(&args[0])
            .map_err(|_| CoreError::native_contract("Invalid asset ID"))?;
        let from = UInt160::from_bytes(&args[1])
            .map_err(|_| CoreError::native_contract("Invalid from address"))?;
        let to = UInt160::from_bytes(&args[2])
            .map_err(|_| CoreError::native_contract("Invalid to address"))?;

        let amount = BigInt::from_signed_bytes_le(&args[3]);

        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "TokenManagement.transfer: amount cannot be negative",
            ));
        }

        if amount.is_zero() {
            return Ok(vec![1]);
        }

        let caller = engine.calling_script_hash();
        if from != caller && !engine.check_witness_hash(&from)? {
            return Ok(vec![0]);
        }

        let context = engine.get_native_storage_context(&self.hash())?;

        let Some(from_state) = self.get_account_state(engine, &asset_id, &from)? else {
            return Ok(vec![0]);
        };

        if from_state.balance < amount {
            return Ok(vec![0]);
        }

        let mut from_balance = from_state.balance;
        from_balance -= &amount;

        let to_state = self
            .get_account_state(engine, &asset_id, &to)?
            .unwrap_or_default();
        let mut to_balance = to_state.balance;
        to_balance += &amount;

        if from_balance.is_zero() {
            let from_key = [
                vec![PREFIX_ACCOUNT_STATE],
                from.to_bytes().to_vec(),
                asset_id.to_bytes().to_vec(),
            ]
            .concat();
            engine.delete_storage_item(&context, &from_key)?;
        } else {
            let from_state = AccountState::with_balance(from_balance);
            self.write_account_state(&context, engine, &from, &asset_id, &from_state)?;
        }

        let to_state = AccountState::with_balance(to_balance);
        self.write_account_state(&context, engine, &to, &asset_id, &to_state)?;

        self.emit_transfer_event(engine, Some(&from), Some(&to), &amount)?;

        Ok(vec![1])
    }
}

#[cfg(test)]
mod tests;
