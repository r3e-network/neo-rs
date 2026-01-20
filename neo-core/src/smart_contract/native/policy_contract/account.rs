//
// account.rs - Account blocking methods for PolicyContract
//

use super::*;

impl PolicyContract {
    pub(super) fn is_blocked(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "isBlocked requires account argument".to_string(),
            ));
        }

        let account_bytes = &args[0];
        if account_bytes.len() != ADDRESS_SIZE {
            return Err(Error::native_contract(format!(
                "Invalid account hash length (must be {ADDRESS_SIZE} bytes)"
            )));
        }
        let account = UInt160::from_bytes(account_bytes)?;
        let context = engine.get_native_storage_context(&self.hash)?;
        let key = Self::blocked_account_suffix(&account);
        let blocked = engine.get_storage_item(&context, &key).is_some();
        Ok(vec![if blocked { 1 } else { 0 }])
    }

    pub(super) fn block_account(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "blockAccount requires account argument".to_string(),
            ));
        }

        let account_bytes = &args[0];
        if account_bytes.len() != ADDRESS_SIZE {
            return Err(Error::native_contract(format!(
                "Invalid account hash length (must be {ADDRESS_SIZE} bytes)"
            )));
        }
        let account = UInt160::from_bytes(account_bytes)?;

        Self::assert_committee(engine)?;
        let blocked = self.block_account_internal(engine, &account)?;

        Ok(vec![if blocked { 1 } else { 0 }])
    }

    pub(super) fn unblock_account(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "unblockAccount requires account argument".to_string(),
            ));
        }

        let account_bytes = &args[0];
        if account_bytes.len() != ADDRESS_SIZE {
            return Err(Error::native_contract(format!(
                "Invalid account hash length (must be {ADDRESS_SIZE} bytes)"
            )));
        }
        let account = UInt160::from_bytes(account_bytes)?;

        Self::assert_committee(engine)?;

        let context = engine.get_native_storage_context(&self.hash)?;
        let key = Self::blocked_account_suffix(&account);
        if engine.get_storage_item(&context, &key).is_none() {
            return Ok(vec![0]);
        }

        engine.delete_storage_item(&context, &key)?;
        Ok(vec![1])
    }

    pub(super) fn get_blocked_accounts(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let context = engine.get_native_storage_context(&self.hash)?;
        let iterator = engine
            .find_storage_entries(
                &context,
                &[Self::PREFIX_BLOCKED_ACCOUNT],
                FindOptions::RemovePrefix | FindOptions::KeysOnly,
            )
            .map_err(Error::native_contract)?;

        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(Error::native_contract)?;
        Ok(iterator_id.to_le_bytes().to_vec())
    }

    /// Blocks an account in the current snapshot (used by other native contracts).
    pub fn block_account_internal(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
    ) -> Result<bool> {
        if engine.is_native_contract_hash(account) {
            return Err(Error::invalid_operation(
                "Cannot block a native contract.".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        let key = Self::blocked_account_suffix(account);
        
        let block_data = engine.get_storage_item(&context, &key);
        if let Some(item) = block_data {
            if item.is_empty() && engine.is_hardfork_enabled(Hardfork::HfFaun) {
                // Set request time for recover funds.
                let timestamp = engine
                    .get_current_block_time()
                    .map_err(|e| Error::invalid_operation(e))?;
                engine.put_storage_item(&context, &key, &Self::encode_u64(timestamp))?;
            }
            return Ok(false);
        }

        if engine.is_hardfork_enabled(Hardfork::HfFaun) {
            use crate::smart_contract::native::neo_token::NeoToken;
             // We ignore the result of VoteInternal as C# does (await and forget return?)
             // C# `await NEO.VoteInternal(...)` returns bool, but BlockAccount continues regardless.
             let _ = NeoToken::new().vote_internal(engine, account, None)?;
        }
        
        // Add to blocked list
        let value = if engine.is_hardfork_enabled(Hardfork::HfFaun) {
            let timestamp = engine
                .get_current_block_time()
                .map_err(|e| Error::invalid_operation(e))?;
            Self::encode_u64(timestamp)
        } else {
            Vec::new()
        };

        engine.put_storage_item(&context, &key, &value)?;
        Ok(true)
    }

    pub(super) fn recover_fund(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::native_contract(
                "recoverFund requires account and token arguments".to_string(),
            ));
        }

        let account = UInt160::from_bytes(&args[0])
            .map_err(|_| Error::native_contract("Invalid account hash".to_string()))?;
        let token = UInt160::from_bytes(&args[1])
            .map_err(|_| Error::native_contract("Invalid token hash".to_string()))?;

        Self::assert_almost_full_committee(engine)?;

        // 2. Check blocked time
        let context = engine.get_native_storage_context(&self.hash)?;
        let key = Self::blocked_account_suffix(&account);
        let block_data = engine.get_storage_item(&context, &key)
            .ok_or_else(|| Error::invalid_operation("Request not found.".to_string()))?;

        // Parse timestamp from block_data
        let request_time = BigInt::from_signed_bytes_le(&block_data)
            .to_u64()
            .unwrap_or(0);

        let current_time = engine
            .get_current_block_time()
            .map_err(|e| Error::invalid_operation(e))?;

        let elapsed = current_time.saturating_sub(request_time);
        if elapsed < Self::REQUIRED_TIME_FOR_RECOVER_FUND_MS {
            let remaining = Self::REQUIRED_TIME_FOR_RECOVER_FUND_MS - elapsed;
            let days = remaining / 86_400_000;
            let hours = (remaining % 86_400_000) / 3_600_000;
            let minutes = (remaining % 3_600_000) / 60_000;
            let seconds = (remaining % 60_000) / 1_000;
            let time_msg = if days > 0 {
                format!("{days}d {hours}h {minutes}m")
            } else if hours > 0 {
                format!("{hours}h {minutes}m {seconds}s")
            } else if minutes > 0 {
                format!("{minutes}m {seconds}s")
            } else {
                format!("{seconds}s")
            };

            return Err(Error::invalid_operation(format!(
                "Request must be signed at least 1 year ago. Remaining time: {time_msg}."
            )));
        }

        // 3. Confirm contract exists and is NEP-17
        let contract_state = crate::smart_contract::native::contract_management::ContractManagement::get_contract_from_snapshot(engine.snapshot_cache().as_ref(), &token)?
             .ok_or_else(|| Error::invalid_operation(format!("Contract {token} does not exist.")))?;
        
        if !contract_state.manifest.supported_standards.contains(&"NEP-17".to_string()) {
             return Err(Error::invalid_operation(format!("Contract {token} does not implement NEP-17 standard.")));
        }

        let original_depth = engine.invocation_stack().len();
        engine.call_from_native_contract_dynamic(
            &account,
            &token,
            "balanceOf",
            vec![StackItem::ByteString(account.to_bytes().to_vec())],
        )?;

        let state = engine.execute_until_invocation_stack_depth(original_depth);
        if state == neo_vm::vm_state::VMState::FAULT {
            let message = engine
                .fault_exception()
                .unwrap_or("VM execution faulted during balanceOf")
                .to_string();
            return Err(Error::invalid_operation(message));
        }

        let stack_len = engine
            .current_evaluation_stack()
            .map(|stack| stack.len())
            .unwrap_or(0);
        let balance_item = if stack_len > 0 {
            engine.pop().map_err(Error::native_contract)?
        } else {
            StackItem::from_int(0)
        };

        let balance = balance_item
            .as_int()
            .map_err(|e| Error::native_contract(format!("Invalid balanceOf result: {e}")))?;

        if balance > BigInt::zero() {
            let original_depth = engine.invocation_stack().len();
            engine.call_from_native_contract_dynamic(
                &account,
                &token,
                "transfer",
                vec![
                    StackItem::ByteString(account.to_bytes().to_vec()),
                    StackItem::ByteString(
                        crate::smart_contract::native::TreasuryContract::new()
                            .hash()
                            .to_bytes()
                            .to_vec(),
                    ),
                    StackItem::from_int(balance.clone()),
                    StackItem::Null,
                ],
            )?;

            let state = engine.execute_until_invocation_stack_depth(original_depth);
            if state == neo_vm::vm_state::VMState::FAULT {
                let message = engine
                    .fault_exception()
                    .unwrap_or("VM execution faulted during transfer")
                    .to_string();
                return Err(Error::invalid_operation(message));
            }

            let stack_len = engine
                .current_evaluation_stack()
                .map(|stack| stack.len())
                .unwrap_or(0);
            let transfer_item = if stack_len > 0 {
                engine.pop().map_err(Error::native_contract)?
            } else {
                StackItem::from_bool(false)
            };

            let transfer_ok = transfer_item
                .as_bool()
                .map_err(|e| Error::native_contract(format!("Invalid transfer result: {e}")))?;

            if !transfer_ok {
                return Err(Error::invalid_operation(format!(
                    "Transfer of {balance} from {account} to {} failed in contract {token}.",
                    crate::smart_contract::native::TreasuryContract::new().hash()
                )));
            }

            engine
                .send_notification(
                    self.hash,
                    "RecoveredFund".to_string(),
                    vec![StackItem::ByteString(account.to_bytes().to_vec())],
                )
                .map_err(Error::native_contract)?;

            return Ok(vec![1]);
        }

        Ok(vec![0])
    }

    pub(super) fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "getFeePerByte" => self.get_fee_per_byte(engine),
            "getExecFeeFactor" => self.get_exec_fee_factor(engine),
            "getExecPicoFeeFactor" => self.get_exec_pico_fee_factor(engine),
            "getStoragePrice" => self.get_storage_price(engine),
            "getMillisecondsPerBlock" => self.get_milliseconds_per_block(engine),
            "getMaxValidUntilBlockIncrement" => self.get_max_valid_until_block_increment(engine),
            "getMaxTraceableBlocks" => self.get_max_traceable_blocks(engine),
            "getAttributeFee" => self.get_attribute_fee(engine, args),
            "setFeePerByte" => self.set_fee_per_byte(engine, args),
            "setExecFeeFactor" => self.set_exec_fee_factor(engine, args),
            "setStoragePrice" => self.set_storage_price(engine, args),
            "setMillisecondsPerBlock" => self.set_milliseconds_per_block(engine, args),
            "setMaxValidUntilBlockIncrement" => {
                self.set_max_valid_until_block_increment(engine, args)
            }
            "setMaxTraceableBlocks" => self.set_max_traceable_blocks(engine, args),
            "setAttributeFee" => self.set_attribute_fee(engine, args),
            "isBlocked" => self.is_blocked(engine, args),
            "blockAccount" => self.block_account(engine, args),
            "unblockAccount" => self.unblock_account(engine, args),
            "getBlockedAccounts" => self.get_blocked_accounts(engine),
            "setWhitelistFeeContract" => self.set_whitelist_fee_contract(engine, args),
            "removeWhitelistFeeContract" => self.remove_whitelist_fee_contract(engine, args),
            "getWhitelistFeeContracts" => self.get_whitelist_fee_contracts(engine),
            "recoverFund" => self.recover_fund(engine, args),
            _ => Err(Error::native_contract(format!("Unknown method: {method}"))),
        }
    }
}
