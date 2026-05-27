use super::{
    AccountState, ID, NFTState, PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX,
    PREFIX_NFT_OWNER_UNIQUE_ID_INDEX, TokenManagement, TokenState, TokenType,
};
use crate::UInt160;
use crate::error::{CoreError, CoreResult};
use crate::persistence::seek_direction::SeekDirection;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::iterators::StorageIterator;
use crate::smart_contract::native::NativeContract;
use crate::smart_contract::native::helpers::parse_uint160_arg;
use crate::smart_contract::{StorageItem, StorageKey};
use num_bigint::BigInt;
use num_traits::Zero;
use std::collections::{BTreeMap, HashSet};

const NFT_INDEX_LOOKUP_PREFIX_LEN: usize = 1 + 20;

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

        let owner = parse_uint160_arg(&args[0], "owner")?;

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

        self.put_token_state(&context, engine, &asset_id, &token_state)?;

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

        let asset_id = parse_uint160_arg(&args[0], "asset ID")?;
        let account = parse_uint160_arg(&args[1], "account")?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let token_key = Self::token_state_key_suffix(&asset_id);

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

        self.put_token_state(&context, engine, &asset_id, &updated_token_state)?;

        let nft_state = NFTState {
            asset_id,
            owner: account,
            properties: Vec::new(),
        };
        self.put_nft_state(&context, engine, &unique_id, &nft_state)?;

        let account_key = Self::account_state_key_suffix(&account, &asset_id);
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

        let nft_id = parse_uint160_arg(&args[0], "NFT ID")?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let nft_key = Self::nft_state_key_suffix(&nft_id);

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

        let token_key = Self::token_state_key_suffix(&nft_state.asset_id);
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
        self.put_token_state(&context, engine, &nft_state.asset_id, &token_state)?;

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

        let nft_id = parse_uint160_arg(&args[0], "NFT ID")?;
        let from = parse_uint160_arg(&args[1], "from")?;
        let to = parse_uint160_arg(&args[2], "to")?;

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
        let nft_key = Self::nft_state_key_suffix(&nft_id);

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
        self.put_nft_state(&context, engine, &nft_id, &nft_state)?;

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

        let nft_id = parse_uint160_arg(&args[0], "NFT ID")?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let nft_key = Self::nft_state_key_suffix(&nft_id);

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

        let asset_id = parse_uint160_arg(&args[0], "asset ID")?;

        self.open_nft_index_iterator(engine, PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX, &asset_id)
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

        let account = parse_uint160_arg(&args[0], "account")?;

        self.open_nft_index_iterator(engine, PREFIX_NFT_OWNER_UNIQUE_ID_INDEX, &account)
    }

    fn open_nft_index_iterator(
        &self,
        engine: &mut ApplicationEngine,
        index_prefix: u8,
        target: &UInt160,
    ) -> CoreResult<Vec<u8>> {
        let filtered = Self::collect_nft_index_entries(engine, index_prefix, target);
        let options = FindOptions::KeysOnly | FindOptions::RemovePrefix;
        let iterator = StorageIterator::new(filtered, NFT_INDEX_LOOKUP_PREFIX_LEN, options);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(CoreError::native_contract)?;

        Ok(iterator_id.to_le_bytes().to_vec())
    }

    fn collect_nft_index_entries(
        engine: &ApplicationEngine,
        index_prefix: u8,
        target: &UInt160,
    ) -> Vec<(StorageKey, StorageItem)> {
        let snapshot = engine.snapshot_cache();
        let prefix = StorageKey::create(ID, index_prefix);

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
            if suffix.is_empty() || suffix[0] != index_prefix {
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

        entries
            .into_iter()
            .filter(|(key, _)| {
                let suffix = key.suffix();
                if suffix.len() < NFT_INDEX_LOOKUP_PREFIX_LEN {
                    return false;
                }
                let indexed_hash =
                    UInt160::from_bytes(&suffix[1..NFT_INDEX_LOOKUP_PREFIX_LEN]).ok();
                indexed_hash.as_ref() == Some(target)
            })
            .collect()
    }
}
