impl TokenManagement {
    fn get_token_state(
        &self,
        engine: &ApplicationEngine,
        asset_id: &UInt160,
    ) -> CoreResult<Option<TokenState>> {
        let snapshot = engine.snapshot_cache();
        let key = Self::token_state_key(asset_id);
        let Some(item) = snapshot.as_ref().try_get(&key) else {
            return Ok(None);
        };
        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(None);
        }
        let token_state = Self::deserialize_interoperable::<TokenState>(&bytes)?;
        Ok(Some(token_state))
    }

    fn get_account_state(
        &self,
        engine: &ApplicationEngine,
        asset_id: &UInt160,
        account: &UInt160,
    ) -> CoreResult<Option<AccountState>> {
        let snapshot = engine.snapshot_cache();
        let key = StorageKey::new(ID, Self::account_state_suffix(account, asset_id));
        let Some(item) = snapshot.as_ref().try_get(&key) else {
            return Ok(None);
        };
        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(None);
        }
        let account_state = Self::deserialize_interoperable::<AccountState>(&bytes)?;
        Ok(Some(account_state))
    }

    fn write_account_state(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        asset_id: &UInt160,
        state: &AccountState,
    ) -> CoreResult<()> {
        let key = Self::account_state_suffix(account, asset_id);
        if state.balance.is_zero() {
            engine.delete_storage_item(context, &key)?;
        } else {
            let bytes = Self::serialize_interoperable(state)?;
            engine.put_storage_item(context, &key, &bytes)?;
        }
        Ok(())
    }

    fn update_account_balance(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        asset_id: &UInt160,
        delta: i32,
    ) -> CoreResult<()> {
        let account_key = Self::account_state_suffix(account, asset_id);

        let mut balance = BigInt::from(0);
        if let Some(account_data) = engine.get_storage_item(context, &account_key) {
            if let Some(state) = Self::deserialize_account_state(&account_data) {
                balance = state.balance;
            }
        }

        balance = balance.clone() + delta;
        if balance.is_zero() {
            engine.delete_storage_item(context, &account_key)?;
        } else if balance.is_negative() {
            return Err(CoreError::native_contract(
                "TokenManagement: account balance cannot be negative",
            ));
        } else {
            let account_state = AccountState::with_balance(balance);
            self.write_account_state(context, engine, account, asset_id, &account_state)?;
        }
        Ok(())
    }

    fn update_nft_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        prefix: u8,
        address: &UInt160,
        nft_id: &UInt160,
        is_add: bool,
    ) -> CoreResult<()> {
        let mut index_key = Vec::with_capacity(NFT_INDEX_KEY_SIZE);
        index_key.push(prefix);
        index_key.extend_from_slice(&address.as_bytes());
        index_key.extend_from_slice(&nft_id.as_bytes());
        let index_key = StorageKey::new(ID, index_key);
        if is_add {
            engine.put_storage_item(context, index_key.suffix(), &[0])?;
        } else {
            engine.delete_storage_item(context, index_key.suffix())?;
        }
        Ok(())
    }

    fn add_nft_to_asset_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        asset_id: &UInt160,
        nft_id: &UInt160,
    ) -> CoreResult<()> {
        self.update_nft_index(
            context,
            engine,
            PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX,
            asset_id,
            nft_id,
            true,
        )
    }

    fn remove_nft_from_asset_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        asset_id: &UInt160,
        nft_id: &UInt160,
    ) -> CoreResult<()> {
        self.update_nft_index(
            context,
            engine,
            PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX,
            asset_id,
            nft_id,
            false,
        )
    }

    fn add_nft_to_owner_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        owner: &UInt160,
        nft_id: &UInt160,
    ) -> CoreResult<()> {
        self.update_nft_index(
            context,
            engine,
            PREFIX_NFT_OWNER_UNIQUE_ID_INDEX,
            owner,
            nft_id,
            true,
        )
    }

    fn remove_nft_from_owner_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        owner: &UInt160,
        nft_id: &UInt160,
    ) -> CoreResult<()> {
        self.update_nft_index(
            context,
            engine,
            PREFIX_NFT_OWNER_UNIQUE_ID_INDEX,
            owner,
            nft_id,
            false,
        )
    }

    fn emit_transfer_event(
        &self,
        engine: &mut ApplicationEngine,
        from: Option<&UInt160>,
        to: Option<&UInt160>,
        amount: &BigInt,
    ) -> CoreResult<()> {
        let from_item = from
            .map(|addr| StackItem::from_byte_string(addr.to_bytes()))
            .unwrap_or_else(StackItem::null);
        let to_item = to
            .map(|addr| StackItem::from_byte_string(addr.to_bytes()))
            .unwrap_or_else(StackItem::null);
        let amount_item = StackItem::from_int(amount.clone());
        engine
            .send_notification(
                self.hash(),
                "Transfer".to_string(),
                vec![from_item, to_item, amount_item],
            )
            .map_err(CoreError::native_contract)
    }

    fn emit_created_event(
        &self,
        engine: &mut ApplicationEngine,
        asset_id: &UInt160,
        token_type: &TokenType,
    ) -> CoreResult<()> {
        let type_value = match token_type {
            TokenType::Fungible => 0,
            TokenType::NonFungible => 1,
        };
        let type_item = StackItem::from_int(type_value);
        let asset_item = StackItem::from_byte_string(asset_id.to_bytes());
        engine
            .send_notification(
                self.hash(),
                "Created".to_string(),
                vec![asset_item, type_item],
            )
            .map_err(CoreError::native_contract)
    }

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
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "TokenManagement.getTokenInfo: invalid arguments",
            ));
        }

        let asset_id = Self::parse_uint160(&args[0], "Invalid asset ID")?;

        let Some(token_state) = self.get_token_state(engine, &asset_id)? else {
            return Err(CoreError::native_contract(
                "TokenManagement.getTokenInfo: token not found",
            ));
        };

        let bytes = Self::serialize_interoperable(&token_state)?;
        Ok(bytes)
    }

    fn invoke_balance_of(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.balanceOf: invalid arguments",
            ));
        }

        let asset_id = Self::parse_uint160(&args[0], "Invalid asset ID")?;
        let account = Self::parse_uint160(&args[1], "Invalid account")?;

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
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "TokenManagement.getAssetsOfOwner: invalid arguments",
            ));
        }

        let account = Self::parse_uint160(&args[0], "Invalid account")?;

        let prefix = StorageKey::create(ID, PREFIX_ACCOUNT_STATE);
        let entries = Self::merge_entries_from_snapshots(engine, &prefix);
        let filtered = Self::filter_entries_by_hash_suffix(entries, 1 + 20 + 20, 1, account);
        Self::store_iterator_id_bytes(
            engine,
            filtered,
            1,
            FindOptions::RemovePrefix | FindOptions::DeserializeValues,
        )
    }

}
