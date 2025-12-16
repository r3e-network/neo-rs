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
        Ok(Self::encode_u32(iterator_id))
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
        if engine.get_storage_item(&context, &key).is_some() {
            return Ok(false);
        }

        engine.put_storage_item(&context, &key, &[])?;
        Ok(true)
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
            _ => Err(Error::native_contract(format!("Unknown method: {method}"))),
        }
    }
}
