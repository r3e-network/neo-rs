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
            .native_err()?;

        let iterator_id = engine
            .store_storage_iterator(iterator)
            .native_err()?;
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

        // v3.9.1: simple existence check — timestamps are set at Faun activation.
        if engine.get_storage_item(&context, &key).is_some() {
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
                .map_err(Error::invalid_operation)?;
            Self::encode_u64(timestamp)
        } else {
            Vec::new()
        };

        engine.put_storage_item(&context, &key, &value)?;
        Ok(true)
    }

    /// Recovers funds from a blocked account after the required waiting period.
    ///
    /// **v3.9.1 note**: This method intentionally performs *read-only* access on
    /// the blocked-account storage entry (via `get_storage_item`).  The entry
    /// must NOT be written back or marked as `Changed` — only the GAS/NEP-17
    /// token transfer is executed.  Pre-Faun entries with empty data are
    /// migrated at the Faun activation block (see `native_impl.rs`).
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
            .map_err(|_| Error::native_contract("Invalid account hash"))?;
        let token = UInt160::from_bytes(&args[1])
            .map_err(|_| Error::native_contract("Invalid token hash"))?;

        Self::assert_almost_full_committee(engine)?;

        // 2. Check blocked time
        let context = engine.get_native_storage_context(&self.hash)?;
        let key = Self::blocked_account_suffix(&account);
        let block_data = engine
            .get_storage_item(&context, &key)
            .ok_or_else(|| Error::invalid_operation("Request not found."))?;

        // Parse timestamp from block_data
        let request_time = BigInt::from_signed_bytes_le(&block_data)
            .to_u64()
            .unwrap_or(0);

        let current_time = engine
            .get_current_block_time()
            .map_err(Error::invalid_operation)?;

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

        if !contract_state
            .manifest
            .supported_standards
            .contains(&"NEP-17".to_string())
        {
            return Err(Error::invalid_operation(format!(
                "Contract {token} does not implement NEP-17 standard."
            )));
        }

        let original_depth = engine.invocation_stack().len();
        engine.call_from_native_contract_dynamic(
            &account,
            &token,
            "balanceOf",
            vec![StackItem::ByteString(account.to_bytes().to_vec())],
        )?;

        let state = engine.execute_until_invocation_stack_depth(original_depth);
        if state == neo_vm_rs::VmState::FAULT {
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
            engine.pop().native_err()?
        } else {
            StackItem::from_int(0)
        };

        let balance = balance_item
            .as_int()
            .map_err(|e| Error::native_contract(format!("Invalid balanceOf result: {e}")))?;

        if balance > num_bigint::BigInt::ZERO {
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
            if state == neo_vm_rs::VmState::FAULT {
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
                engine.pop().native_err()?
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
                .native_err()?;

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
        self.dispatch_method(engine, method, args)
    }
}
