//
// nep17.rs - NEP-17 fungible token standard implementation
//

use super::*;

/// NEP-17 and governance method implementations
impl NeoToken {
    /// Encodes a BigInt amount to bytes (C# compatible format)
    pub(super) fn encode_amount(value: &BigInt) -> Vec<u8> {
        let mut bytes = value.to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    /// Decodes bytes to a BigInt amount
    pub(super) fn decode_amount(data: &[u8]) -> BigInt {
        BigInt::from_signed_bytes_le(data)
    }

    /// Reads an account UInt160 from argument bytes
    pub(super) fn read_account(&self, data: &[u8]) -> CoreResult<UInt160> {
        if data.len() != 20 {
            return Err(CoreError::native_contract(
                "Account argument must be 20 bytes".to_string(),
            ));
        }
        UInt160::from_bytes(data).map_err(|err| CoreError::native_contract(err.to_string()))
    }

    /// Reads an ECPoint public key from argument bytes
    pub(super) fn read_public_key(&self, data: &[u8]) -> CoreResult<ECPoint> {
        if data.len() != 33 {
            return Err(CoreError::native_contract(
                "Public key argument must be 33 bytes".to_string(),
            ));
        }
        ECPoint::from_bytes(data).map_err(|err| CoreError::native_contract(err.to_string()))
    }

    /// balanceOf implementation - returns NEO balance of an account
    pub(super) fn balance_of(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "balanceOf expects exactly one argument".to_string(),
            ));
        }
        let account = self.read_account(&args[0])?;
        let snapshot = engine.snapshot_cache();
        let balance = self.balance_of_snapshot(snapshot.as_ref(), &account)?;
        Ok(Self::encode_amount(&balance))
    }

    /// transfer implementation - transfers NEO between accounts
    pub(super) fn transfer(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() != 4 {
            return Err(CoreError::native_contract(
                "transfer expects from, to, amount, data arguments".to_string(),
            ));
        }

        let from = self.read_account(&args[0])?;
        let to = self.read_account(&args[1])?;
        let amount = Self::decode_amount(&args[2]);
        let data_bytes = args[3].clone();
        let data_item = if data_bytes.is_empty() {
            StackItem::null()
        } else {
            StackItem::from_byte_string(data_bytes)
        };

        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "Amount cannot be negative".to_string(),
            ));
        }

        let caller = engine.calling_script_hash();
        if from != caller && !engine.check_witness_hash(&from)? {
            return Ok(vec![0]);
        }

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let context = engine.get_native_storage_context(&self.hash())?;

        let mut gas_distributions: Vec<(UInt160, BigInt)> = Vec::new();

        let mut from_state_opt = self.get_account_state(snapshot_ref, &from)?;

        if amount.is_zero() {
            if let Some(mut state_from) = from_state_opt {
                if let Some(reward) = self.on_balance_changing(
                    engine,
                    &from,
                    &mut state_from,
                    &BigInt::zero(),
                    &context,
                )? {
                    gas_distributions.push((from, reward));
                }
                self.write_account_state(&context, engine, &from, &state_from)?;
            }
        } else {
            let mut state_from = match from_state_opt.take() {
                Some(state) => state,
                None => return Ok(vec![0]),
            };

            if state_from.balance < amount {
                return Ok(vec![0]);
            }

            if from == to {
                if let Some(reward) = self.on_balance_changing(
                    engine,
                    &from,
                    &mut state_from,
                    &BigInt::zero(),
                    &context,
                )? {
                    gas_distributions.push((from, reward));
                }
                self.write_account_state(&context, engine, &from, &state_from)?;
            } else {
                let neg_amount = -amount.clone();
                if let Some(reward) =
                    self.on_balance_changing(engine, &from, &mut state_from, &neg_amount, &context)?
                {
                    gas_distributions.push((from, reward));
                }

                state_from.balance -= &amount;
                if state_from.balance.is_zero() {
                    self.delete_account_state(&context, engine, &from)?;
                } else {
                    self.write_account_state(&context, engine, &from, &state_from)?;
                }

                let mut state_to = self
                    .get_account_state(snapshot_ref, &to)?
                    .unwrap_or_default();
                if let Some(reward) =
                    self.on_balance_changing(engine, &to, &mut state_to, &amount, &context)?
                {
                    gas_distributions.push((to, reward));
                }
                state_to.balance += &amount;
                self.write_account_state(&context, engine, &to, &state_to)?;
            }
        }

        self.emit_transfer_event(engine, Some(&from), Some(&to), &amount)?;

        if ContractManagement::get_contract_from_snapshot(snapshot_ref, &to)
            .map_err(|e| CoreError::native_contract(e.to_string()))?
            .is_some()
        {
            engine.queue_contract_call_from_native(
                self.hash(),
                to,
                "onNEP17Payment",
                vec![
                    StackItem::from_byte_string(from.to_bytes()),
                    StackItem::from_int(amount.clone()),
                    data_item,
                ],
            );
        }

        if !gas_distributions.is_empty() {
            let gas_token = GasToken::new();
            for (account, datoshi) in gas_distributions {
                gas_token.mint(engine, &account, &datoshi, true)?;
            }
        }

        Ok(vec![1])
    }

    pub(super) fn delete_account_state(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
    ) -> CoreResult<()> {
        let key = StorageKey::create_with_uint160(Self::ID, PREFIX_ACCOUNT, account)
            .suffix()
            .to_vec();
        engine.delete_storage_item(context, &key)?;
        Ok(())
    }

    /// Writes account state to storage (C# NeoAccountState interoperable format).
    pub(super) fn write_account_state(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        state: &NeoAccountState,
    ) -> CoreResult<()> {
        let key = StorageKey::create_with_uint160(Self::ID, PREFIX_ACCOUNT, account)
            .suffix()
            .to_vec();
        if state.balance.is_zero() {
            engine.delete_storage_item(context, &key)?;
        } else {
            let stack_item = state.to_stack_item();
            let bytes = BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
                .map_err(CoreError::native_contract)?;
            engine.put_storage_item(context, &key, &bytes)?;
        }
        Ok(())
    }

    /// Mirrors C# NeoToken.DistributeGas. Updates balance height/last gas per vote and
    /// returns the unclaimed GAS in datoshi that should be minted to the account.
    pub(super) fn distribute_gas(
        &self,
        engine: &mut ApplicationEngine,
        _account: &UInt160,
        state: &mut NeoAccountState,
    ) -> CoreResult<Option<BigInt>> {
        let persisting_index = match engine.persisting_block() {
            Some(block) => block.index(),
            None => return Ok(None),
        };

        let snapshot = engine.snapshot_cache();
        let reward = self.calculate_bonus(snapshot.as_ref(), state, persisting_index)?;

        state.balance_height = persisting_index;
        if let Some(vote_to) = state.vote_to.as_ref() {
            let latest = self.latest_gas_per_vote(snapshot.as_ref(), vote_to);
            state.last_gas_per_vote = latest;
        }

        if reward.is_zero() {
            Ok(None)
        } else {
            Ok(Some(reward))
        }
    }

    /// Mirrors C# NeoToken.OnBalanceChanging.
    pub(super) fn on_balance_changing(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        state: &mut NeoAccountState,
        amount: &BigInt,
        context: &StorageContext,
    ) -> CoreResult<Option<BigInt>> {
        let distribution = self.distribute_gas(engine, account, state)?;

        if amount.is_zero() {
            return Ok(distribution);
        }

        let Some(vote_to) = state.vote_to.clone() else {
            return Ok(distribution);
        };

        // Update voters count.
        let snapshot = engine.snapshot_cache();
        let voters_key = StorageKey::create(Self::ID, Self::PREFIX_VOTERS_COUNT);
        let current_voters = snapshot
            .as_ref()
            .try_get(&voters_key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(BigInt::zero);
        let updated_voters = current_voters + amount;
        let voters_suffix = voters_key.suffix().to_vec();
        engine.put_storage_item(
            context,
            &voters_suffix,
            &Self::encode_amount(&updated_voters),
        )?;

        // Update candidate votes.
        let mut candidate = self
            .get_candidate_state(snapshot.as_ref(), &vote_to)?
            .unwrap_or_default();
        candidate.votes += amount;
        self.write_candidate_state(context, engine, &vote_to, &candidate)?;

        Ok(distribution)
    }

    /// Emits Transfer event
    pub(super) fn emit_transfer_event(
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
}
