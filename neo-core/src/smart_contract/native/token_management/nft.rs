use super::{
    AccountState, NFTState, TokenManagement, TokenState, TokenType, ID, PREFIX_ACCOUNT_STATE,
    PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX, PREFIX_NFT_OWNER_UNIQUE_ID_INDEX, PREFIX_NFT_STATE,
    PREFIX_TOKEN_STATE,
};
use crate::error::{CoreError, CoreResult};
use crate::persistence::seek_direction::SeekDirection;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::iterators::StorageIterator;
use crate::smart_contract::native::NativeContract;
use crate::smart_contract::{StorageItem, StorageKey};
use crate::UInt160;
use num_bigint::BigInt;
use num_traits::Zero;
use std::collections::{BTreeMap, HashSet};

impl TokenManagement {
    pub(super) fn invoke_create_non_fungible(
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

    pub(super) fn invoke_mint_nft(
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

    pub(super) fn invoke_burn_nft(
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

    pub(super) fn invoke_transfer_nft(
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

    pub(super) fn invoke_get_nft_info(
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

    pub(super) fn invoke_get_nfts(
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

        let mut entries_map = BTreeMap::new();
        let mut snapshot_keys: HashSet<Vec<u8>> = HashSet::new();

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

    pub(super) fn invoke_get_nfts_of_owner(
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

        let mut entries_map = BTreeMap::new();
        let mut snapshot_keys: HashSet<Vec<u8>> = HashSet::new();

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
