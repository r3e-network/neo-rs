impl TokenManagement {
    fn invoke_create(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 7 {
            return Err(CoreError::native_contract(
                "TokenManagement.create: invalid arguments",
            ));
        }

        let token_type = if args[0].is_empty() || args[0][0] == 0 {
            TokenType::Fungible
        } else {
            TokenType::NonFungible
        };

        let owner = Self::parse_uint160(&args[1], "Invalid owner")?;

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

        self.write_token_state(&context, engine, &asset_id, &token_state)?;

        self.emit_created_event(engine, &asset_id, &token_type)?;

        Ok(asset_id.to_bytes().to_vec())
    }

    fn invoke_create_non_fungible(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 4 {
            return Err(CoreError::native_contract(
                "TokenManagement.createNonFungible: invalid arguments",
            ));
        }

        let owner = Self::parse_uint160(&args[0], "Invalid owner")?;

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

        self.write_token_state(&context, engine, &asset_id, &token_state)?;

        self.emit_created_event(engine, &asset_id, &TokenType::NonFungible)?;

        Ok(asset_id.to_bytes().to_vec())
    }

    fn invoke_mint(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.mint: invalid arguments",
            ));
        }

        let asset_id = Self::parse_uint160(&args[0], "Invalid asset ID")?;
        let account = Self::parse_uint160(&args[1], "Invalid account")?;

        let amount = Self::parse_non_negative_optional_amount(
            args,
            2,
            1,
            "TokenManagement.mint: amount cannot be negative",
        )?;

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

        self.write_token_state(&context, engine, &asset_id, &token_state)?;

        self.emit_transfer_event(engine, None, Some(&account), &amount)?;

        Ok(vec![1])
    }

    fn invoke_burn(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "TokenManagement.burn: invalid arguments",
            ));
        }

        let asset_id = Self::parse_uint160(&args[0], "Invalid asset ID")?;
        let account = Self::parse_uint160(&args[1], "Invalid account")?;

        let amount = Self::parse_non_negative_optional_amount(
            args,
            2,
            1,
            "TokenManagement.burn: amount cannot be negative",
        )?;

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
            let asset_key = Self::account_state_suffix(&account, &asset_id);
            engine.delete_storage_item(&context, &asset_key)?;
        } else {
            self.write_account_state(&context, engine, &account, &asset_id, &account_state)?;
        }

        self.write_token_state(&context, engine, &asset_id, &token_state)?;

        self.emit_transfer_event(engine, Some(&account), None, &amount)?;

        Ok(vec![1])
    }

    fn invoke_transfer(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 4 {
            return Err(CoreError::native_contract(
                "TokenManagement.transfer: invalid arguments",
            ));
        }

        let asset_id = Self::parse_uint160(&args[0], "Invalid asset ID")?;
        let from = Self::parse_uint160(&args[1], "Invalid from address")?;
        let to = Self::parse_uint160(&args[2], "Invalid to address")?;

        let amount = Self::parse_non_negative_amount(
            &args[3],
            "TokenManagement.transfer: amount cannot be negative",
        )?;

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
            let from_key = Self::account_state_suffix(&from, &asset_id);
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
