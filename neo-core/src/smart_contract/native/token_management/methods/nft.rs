impl TokenManagement {
    fn invoke_mint_nft(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.mintNFT: invalid arguments",
            ));
        }

        let asset_id = Self::parse_uint160(&args[0], "Invalid asset ID")?;
        let account = Self::parse_uint160(&args[1], "Invalid account")?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let token_key = Self::token_state_suffix(&asset_id);

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

        self.write_token_state(&context, engine, &asset_id, &updated_token_state)?;

        let nft_state = NFTState {
            asset_id,
            owner: account,
            properties: Vec::new(),
        };
        self.write_nft_state(&context, engine, &unique_id, &nft_state)?;

        let account_key = Self::account_state_suffix(&account, &asset_id);
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
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "TokenManagement.burnNFT: invalid arguments",
            ));
        }

        let nft_id = Self::parse_uint160(&args[0], "Invalid NFT ID")?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let nft_key = Self::nft_state_suffix(&nft_id);

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

        let token_key = Self::token_state_suffix(&nft_state.asset_id);
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
        self.write_token_state(&context, engine, &nft_state.asset_id, &token_state)?;

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
        if args.len() < 4 {
            return Err(CoreError::native_contract(
                "TokenManagement.transferNFT: invalid arguments",
            ));
        }

        let nft_id = Self::parse_uint160(&args[0], "Invalid NFT ID")?;
        let from = Self::parse_uint160(&args[1], "Invalid from")?;
        let to = Self::parse_uint160(&args[2], "Invalid to")?;

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
        let nft_key = Self::nft_state_suffix(&nft_id);

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
        self.write_nft_state(&context, engine, &nft_id, &nft_state)?;

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
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "TokenManagement.getNFTInfo: invalid arguments",
            ));
        }

        let nft_id = Self::parse_uint160(&args[0], "Invalid NFT ID")?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let nft_key = Self::nft_state_suffix(&nft_id);

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
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "TokenManagement.getNFTs: invalid arguments",
            ));
        }

        let asset_id = Self::parse_uint160(&args[0], "Invalid asset ID")?;

        let prefix = StorageKey::create(ID, PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX);
        let entries = Self::merge_entries_from_snapshots_with_tracking(
            engine,
            &prefix,
            PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX,
        );
        let filtered = Self::filter_entries_by_hash_suffix(entries, 1 + 20, 1, asset_id);
        Self::store_iterator_id_bytes(
            engine,
            filtered,
            21,
            FindOptions::KeysOnly | FindOptions::RemovePrefix,
        )
    }

    fn invoke_get_nfts_of_owner(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "TokenManagement.getNFTsOfOwner: invalid arguments",
            ));
        }

        let account = Self::parse_uint160(&args[0], "Invalid account")?;

        let prefix = StorageKey::create(ID, PREFIX_NFT_OWNER_UNIQUE_ID_INDEX);
        let entries = Self::merge_entries_from_snapshots_with_tracking(
            engine,
            &prefix,
            PREFIX_NFT_OWNER_UNIQUE_ID_INDEX,
        );
        let filtered = Self::filter_entries_by_hash_suffix(entries, 1 + 20, 1, account);
        Self::store_iterator_id_bytes(
            engine,
            filtered,
            21,
            FindOptions::KeysOnly | FindOptions::RemovePrefix,
        )
    }

    fn get_next_nft_unique_id(&self, engine: &mut ApplicationEngine) -> CoreResult<UInt160> {
        let context = engine.get_native_storage_context(&self.hash())?;
        let seed_key = StorageKey::create(ID, PREFIX_NFT_UNIQUE_ID_SEED)
            .suffix()
            .to_vec();

        let seed = match engine.get_storage_item(&context, &seed_key) {
            Some(data) => BigInt::from_signed_bytes_be(&data),
            None => BigInt::from(0),
        };

        let new_seed = seed + 1;
        let seed_bytes = Self::encode_bigint(&new_seed);
        engine.put_storage_item(&context, &seed_key, &seed_bytes)?;

        let block_hash = match engine.persisting_block() {
            Some(block) => block.hash(),
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.getNextNFTUniqueId: no persisting block",
                ));
            }
        };

        let mut buffer = Vec::with_capacity(32 + seed_bytes.len());
        buffer.extend_from_slice(&block_hash.as_bytes());
        buffer.extend_from_slice(&seed_bytes);
        let hash = NeoHash::hash160(&buffer);
        let unique_id = UInt160::from_bytes(&hash).unwrap_or_default();
        Ok(unique_id)
    }
}
