//! Policy native-method dispatch.
//!
//! This module keeps the long method-name switch out of the contract root while
//! preserving the exact C#-compatible storage, notification, and hardfork
//! semantics for each Policy method.

use super::storage::WhitelistedContractView;
use super::{
    FEE_FACTOR, MAX_ATTRIBUTE_FEE, POLICY_MILLISECONDS_PER_BLOCK_CHANGED_EVENT,
    POLICY_RECOVERED_FUND_EVENT, POLICY_WHITELIST_FEE_CHANGED_EVENT, PolicyContract,
    REQUIRED_TIME_FOR_RECOVER_FUND,
};
use crate::committee::assert_committee;
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_primitives::FindOptions;
use neo_storage::StorageItem;
use neo_vm::StackItem;
use num_bigint::BigInt;

impl PolicyContract {
    pub(super) fn invoke_policy_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getFeePerByte" => Ok(BigInt::from(self.fee_per_byte(&snapshot)?).to_signed_bytes_le()),
            "getStoragePrice" => {
                Ok(BigInt::from(self.storage_price(&snapshot)?).to_signed_bytes_le())
            }
            "setFeePerByte" => {
                // C# order: validate range, then AssertCommittee, then write.
                let value = crate::args::raw_i64_arg(args, 0, "PolicyContract::setFeePerByte")?;
                Self::validate_fee_per_byte(value)?;
                assert_committee(engine, "setFeePerByte")?;
                self.put_fee_per_byte(&engine.snapshot_cache(), value)?;
                Ok(Vec::new())
            }
            "setStoragePrice" => {
                let value = crate::args::raw_i64_arg(args, 0, "PolicyContract::setStoragePrice")?;
                Self::validate_storage_price(value)?;
                assert_committee(engine, "setStoragePrice")?;
                self.put_storage_price(&engine.snapshot_cache(), value)?;
                Ok(Vec::new())
            }
            "getExecFeeFactor" => {
                // C#: from HF_Faun the stored value is pico-GAS, so divide it out;
                // before the configured Faun height return it raw.
                let raw = self.exec_fee_factor_raw(&snapshot)?;
                let value = if engine.is_hardfork_enabled(Hardfork::HfFaun) {
                    raw / FEE_FACTOR
                } else {
                    raw
                };
                Ok(BigInt::from(value).to_signed_bytes_le())
            }
            "getExecPicoFeeFactor" => {
                // C# (HF_Faun): the raw stored pico-GAS value, undivided.
                Ok(BigInt::from(self.exec_fee_factor_raw(&snapshot)?).to_signed_bytes_le())
            }
            "setExecFeeFactor" => {
                let value = crate::args::raw_i64_arg(args, 0, "PolicyContract::setExecFeeFactor")?;
                self.validate_exec_fee_factor(engine, value)?;
                assert_committee(engine, "setExecFeeFactor")?;
                self.put_exec_fee_factor(&engine.snapshot_cache(), value)?;
                Ok(Vec::new())
            }
            "getAttributeFee" => {
                // C# V0/V1: allowNotaryAssisted is exactly "HF_Echidna enabled".
                let attribute_type =
                    crate::args::raw_u8_arg(args, 0, "PolicyContract::getAttributeFee")?;
                let allow_notary = engine.is_hardfork_enabled(Hardfork::HfEchidna);
                let fee = self.attribute_fee(&snapshot, attribute_type, allow_notary)?;
                Ok(BigInt::from(fee).to_signed_bytes_le())
            }
            "setAttributeFee" => {
                // C#: validate type (NotaryAssisted gated by HF_Echidna), then
                // value <= MaxAttributeFee, then AssertCommittee, then write.
                let attribute_type =
                    crate::args::raw_u8_arg(args, 0, "PolicyContract::setAttributeFee")?;
                let value = crate::args::raw_u32_arg(args, 1, "PolicyContract::setAttributeFee")?;
                let allow_notary = engine.is_hardfork_enabled(Hardfork::HfEchidna);
                Self::validate_attribute_type(attribute_type, allow_notary)?;
                if i64::from(value) > MAX_ATTRIBUTE_FEE {
                    return Err(CoreError::invalid_operation(format!(
                        "AttributeFee must be less than {MAX_ATTRIBUTE_FEE}, got {value}"
                    )));
                }
                assert_committee(engine, "setAttributeFee")?;
                self.put_attribute_fee(&engine.snapshot_cache(), attribute_type, i64::from(value));
                Ok(Vec::new())
            }
            "getBlockedAccounts" => {
                // C# GetBlockedAccounts: an iterator over Prefix_BlockedAccount with
                // FindOptions.RemovePrefix | KeysOnly and prefix length 1, yielding
                // the blocked account hashes (keys only). The 4-byte iterator id is
                // decoded back into an InteropInterface by the dispatcher.
                let results = self.blocked_account_entries(&snapshot);
                let iterator_id = engine
                    .create_storage_iterator_with_options(
                        results,
                        1,
                        FindOptions::RemovePrefix | FindOptions::KeysOnly,
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "PolicyContract::getBlockedAccounts: {e}"
                        ))
                    })?;
                Ok(iterator_id.to_le_bytes().to_vec())
            }
            "isBlocked" => {
                let account = crate::args::raw_account(args, "PolicyContract::isBlocked")?;
                // C# IsBlocked = snapshot.Contains(key(Prefix_BlockedAccount, account)).
                let blocked = snapshot.get(&Self::blocked_account_key(&account)).is_some();
                Ok(vec![u8::from(blocked)])
            }
            "unblockAccount" => {
                // C#: AssertCommittee -> if not blocked return false ->
                // delete the entry -> return true.
                let account = crate::args::raw_account(args, "PolicyContract::unblockAccount")?;
                assert_committee(engine, "unblockAccount")?;
                let key = Self::blocked_account_key(&account);
                let snapshot = engine.snapshot_cache();
                let was_blocked = snapshot.get(&key).is_some();
                if was_blocked {
                    snapshot.delete(&key);
                }
                Ok(vec![u8::from(was_blocked)])
            }
            "getMillisecondsPerBlock" => {
                Ok(BigInt::from(self.read_milliseconds_per_block(engine)?).to_signed_bytes_le())
            }
            "setMillisecondsPerBlock" => {
                // C#: validate range -> AssertCommittee -> read old -> write ->
                // emit MillisecondsPerBlockChanged[oldValue, newValue].
                let value =
                    crate::args::raw_i64_arg(args, 0, "PolicyContract::setMillisecondsPerBlock")?;
                Self::validate_milliseconds_per_block(value)?;
                assert_committee(engine, "setMillisecondsPerBlock")?;
                let old = self.read_milliseconds_per_block(engine)?;
                self.put_milliseconds_per_block(&engine.snapshot_cache(), value)?;
                engine
                    .send_notification(
                        Self::script_hash(),
                        POLICY_MILLISECONDS_PER_BLOCK_CHANGED_EVENT.to_owned(),
                        vec![StackItem::from_int(old), StackItem::from_int(value)],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("setMillisecondsPerBlock notify: {e}"))
                    })?;
                Ok(Vec::new())
            }
            "setMaxValidUntilBlockIncrement" => {
                // C#: range [1, 86400] -> value < MaxTraceableBlocks -> committee.
                let value = crate::args::raw_i64_arg(
                    args,
                    0,
                    "PolicyContract::setMaxValidUntilBlockIncrement",
                )?;
                Self::validate_max_valid_until_block_increment(value)?;
                let mtb = self.max_traceable_blocks(engine)? as i64;
                if value >= mtb {
                    return Err(CoreError::invalid_operation(format!(
                        "MaxValidUntilBlockIncrement must be lower than MaxTraceableBlocks ({value} vs {mtb})"
                    )));
                }
                assert_committee(engine, "setMaxValidUntilBlockIncrement")?;
                self.put_max_valid_until_block_increment(&engine.snapshot_cache(), value)?;
                Ok(Vec::new())
            }
            "setMaxTraceableBlocks" => {
                // C#: range [1, 2102400] -> can only decrease -> value >
                // MaxValidUntilBlockIncrement -> committee.
                let value =
                    crate::args::raw_i64_arg(args, 0, "PolicyContract::setMaxTraceableBlocks")?;
                Self::validate_max_traceable_blocks(value)?;
                let old = self.max_traceable_blocks(engine)? as i64;
                if value > old {
                    return Err(CoreError::invalid_operation(format!(
                        "MaxTraceableBlocks can not be increased (old {old}, new {value})"
                    )));
                }
                let mvub = self.read_max_valid_until_block_increment(engine)?;
                if value <= mvub {
                    return Err(CoreError::invalid_operation(format!(
                        "MaxTraceableBlocks must be larger than MaxValidUntilBlockIncrement ({value} vs {mvub})"
                    )));
                }
                assert_committee(engine, "setMaxTraceableBlocks")?;
                self.put_max_traceable_blocks(&engine.snapshot_cache(), value)?;
                Ok(Vec::new())
            }
            "getMaxValidUntilBlockIncrement" => Ok(BigInt::from(
                self.read_max_valid_until_block_increment(engine)?,
            )
            .to_signed_bytes_le()),
            "getMaxTraceableBlocks" => {
                Ok(BigInt::from(self.max_traceable_blocks(engine)? as i64).to_signed_bytes_le())
            }
            "blockAccount" => {
                // C# BlockAccountV0/V1 (identical bodies; only the manifest call
                // flags differ): AssertCommittee, then BlockAccountInternal.
                let account = crate::args::raw_hash160(args, 0, "PolicyContract::blockAccount")?;
                assert_committee(engine, "blockAccount")?;
                Ok(vec![u8::from(
                    self.block_account_internal(engine, &account)?,
                )])
            }
            "setWhitelistFeeContract" => {
                // C# SetWhitelistFeeContract: ThrowIfNegative(fixedFee) ->
                // CheckCommittee -> GetContract -> resolve the (method, argCount)
                // descriptor -> upsert WhitelistedContract (only FixedFee changes
                // on an existing entry) -> notify WhitelistFeeChanged.
                let contract_hash =
                    crate::args::raw_hash160(args, 0, "PolicyContract::setWhitelistFeeContract")?;
                let method_name = crate::args::raw_string_arg(
                    args,
                    1,
                    "PolicyContract::setWhitelistFeeContract",
                    "method name",
                )?;
                let arg_count =
                    crate::args::raw_i32_arg(args, 2, "PolicyContract::setWhitelistFeeContract")?;
                let fixed_fee =
                    crate::args::raw_i64_arg(args, 3, "PolicyContract::setWhitelistFeeContract")?;
                if fixed_fee < 0 {
                    return Err(CoreError::invalid_operation(format!(
                        "fixedFee ('{fixed_fee}') must be a non-negative value."
                    )));
                }
                assert_committee(engine, "setWhitelistFeeContract")?;
                let snapshot = engine.snapshot_cache();
                let offset = self.resolve_whitelist_method_offset(
                    &snapshot,
                    &contract_hash,
                    &method_name,
                    arg_count,
                )?;
                let key = Self::whitelist_fee_key(&contract_hash, offset);
                let view = match snapshot.get(&key) {
                    // GetAndChange on an existing entry mutates FixedFee only.
                    Some(item) => {
                        let mut view = Self::decode_whitelisted_contract(&item.value_bytes())?;
                        view.fixed_fee = fixed_fee;
                        view
                    }
                    None => WhitelistedContractView {
                        contract_hash,
                        method: method_name.clone(),
                        arg_count,
                        fixed_fee,
                    },
                };
                snapshot.update(
                    key,
                    StorageItem::from_bytes(Self::encode_whitelisted_contract(&view)?),
                );
                engine
                    .send_notification(
                        Self::script_hash(),
                        POLICY_WHITELIST_FEE_CHANGED_EVENT.to_owned(),
                        vec![
                            StackItem::from_byte_string(contract_hash.to_bytes()),
                            StackItem::from_byte_string(method_name.into_bytes()),
                            StackItem::from_int(BigInt::from(arg_count)),
                            StackItem::from_int(BigInt::from(fixed_fee)),
                        ],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("setWhitelistFeeContract notify: {e}"))
                    })?;
                Ok(Vec::new())
            }
            "removeWhitelistFeeContract" => {
                // C# RemoveWhitelistFeeContract: CheckCommittee -> GetContract ->
                // resolve the descriptor -> fault when no whitelist entry exists
                // -> delete -> notify WhitelistFeeChanged with a null fee.
                let contract_hash = crate::args::raw_hash160(
                    args,
                    0,
                    "PolicyContract::removeWhitelistFeeContract",
                )?;
                let method_name = crate::args::raw_string_arg(
                    args,
                    1,
                    "PolicyContract::removeWhitelistFeeContract",
                    "method name",
                )?;
                let arg_count = crate::args::raw_i32_arg(
                    args,
                    2,
                    "PolicyContract::removeWhitelistFeeContract",
                )?;
                assert_committee(engine, "removeWhitelistFeeContract")?;
                let snapshot = engine.snapshot_cache();
                let offset = self.resolve_whitelist_method_offset(
                    &snapshot,
                    &contract_hash,
                    &method_name,
                    arg_count,
                )?;
                let key = Self::whitelist_fee_key(&contract_hash, offset);
                if snapshot.get(&key).is_none() {
                    return Err(CoreError::invalid_operation("Whitelist not found"));
                }
                snapshot.delete(&key);
                engine
                    .send_notification(
                        Self::script_hash(),
                        POLICY_WHITELIST_FEE_CHANGED_EVENT.to_owned(),
                        vec![
                            StackItem::from_byte_string(contract_hash.to_bytes()),
                            StackItem::from_byte_string(method_name.into_bytes()),
                            StackItem::from_int(BigInt::from(arg_count)),
                            StackItem::null(),
                        ],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "removeWhitelistFeeContract notify: {e}"
                        ))
                    })?;
                Ok(Vec::new())
            }
            "getWhitelistFeeContracts" => {
                // C# GetWhitelistFeeContracts: an iterator over
                // Prefix_WhitelistedFeeContracts with FindOptions.RemovePrefix |
                // ValuesOnly | DeserializeValues and prefix length 1, yielding the
                // deserialized WhitelistedContract structs.
                let results = self.whitelist_fee_entries(&snapshot);
                let iterator_id = engine
                    .create_storage_iterator_with_options(
                        results,
                        1,
                        FindOptions::RemovePrefix
                            | FindOptions::ValuesOnly
                            | FindOptions::DeserializeValues,
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "PolicyContract::getWhitelistFeeContracts: {e}"
                        ))
                    })?;
                Ok(iterator_id.to_le_bytes().to_vec())
            }
            "recoverFund" => {
                // C# RecoverFund: AssertAlmostFullCommittee -> the blocked-account
                // entry is the request record (fault "Request not found.") -> at
                // least 1 year must have elapsed since its timestamp -> the token
                // must be a deployed NEP-17 contract -> sweep the account's
                // balance to Treasury via balanceOf/transfer.
                let account = crate::args::raw_hash160(args, 0, "PolicyContract::recoverFund")?;
                let token = crate::args::raw_hash160(args, 1, "PolicyContract::recoverFund")?;
                self.assert_almost_full_committee(engine)?;

                let snapshot = engine.snapshot_cache();
                let entry = snapshot
                    .get(&Self::blocked_account_key(&account))
                    .ok_or_else(|| CoreError::invalid_operation("Request not found."))?;
                let request_time = BigInt::from_signed_bytes_le(&entry.value_bytes());
                let now = BigInt::from(engine.current_block_timestamp()?);
                let elapsed = now - request_time;
                let required = BigInt::from(REQUIRED_TIME_FOR_RECOVER_FUND);
                if elapsed < required {
                    let remaining = required - elapsed;
                    return Err(CoreError::invalid_operation(format!(
                        "Request must be signed at least 1 year ago. Remaining time: {}.",
                        Self::format_remaining_time(&remaining)
                    )));
                }

                let contract =
                    crate::ContractManagement::get_contract_from_snapshot(&snapshot, &token)?
                        .ok_or_else(|| {
                            CoreError::invalid_operation(format!(
                                "Contract {token} does not exist."
                            ))
                        })?;
                if !contract
                    .manifest
                    .supported_standards
                    .iter()
                    .any(|s| s == "NEP-17")
                {
                    return Err(CoreError::invalid_operation(format!(
                        "Contract {token} does not implement NEP-17 standard."
                    )));
                }

                // C# PolicyContract.RecoverFund sweep: `await
                // engine.CallFromNativeContractAsync<BigInteger>(account, token,
                // "balanceOf", account)` - the callee runs through the VM with
                // `account` as the native calling script hash, so the token's
                // `from == CallingScriptHash` witness bypass authorizes the
                // transfer without the account's signature.
                let balance = engine
                    .call_from_native_contract_returning(
                        &account,
                        &token,
                        "balanceOf",
                        vec![StackItem::from_byte_string(account.to_bytes())],
                    )?
                    .as_int()
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("recoverFund: balanceOf result: {e}"))
                    })?;

                if balance > BigInt::from(0) {
                    // C#: `await engine.CallFromNativeContractAsync<bool>(account,
                    // token, "transfer", account, Treasury.Hash, balance,
                    // StackItem.Null)`; a `false` result faults.
                    let transferred = engine
                        .call_from_native_contract_returning(
                            &account,
                            &token,
                            "transfer",
                            vec![
                                StackItem::from_byte_string(account.to_bytes()),
                                StackItem::from_byte_string(
                                    crate::hashes::TREASURY_HASH.to_bytes(),
                                ),
                                StackItem::from_int(balance.clone()),
                                StackItem::null(),
                            ],
                        )?
                        .as_bool()
                        .map_err(|e| {
                            CoreError::invalid_operation(format!(
                                "recoverFund: transfer result: {e}"
                            ))
                        })?;
                    if !transferred {
                        return Err(CoreError::invalid_operation(format!(
                            "Transfer of {balance} from {account} to {} failed in contract {token}.",
                            *crate::hashes::TREASURY_HASH
                        )));
                    }
                    // C#: engine.SendNotification(Hash, "RecoveredFund",
                    // [ByteString(account)]).
                    engine
                        .send_notification(
                            Self::script_hash(),
                            POLICY_RECOVERED_FUND_EVENT.to_owned(),
                            vec![StackItem::from_byte_string(account.to_bytes())],
                        )
                        .map_err(|e| {
                            CoreError::invalid_operation(format!("recoverFund notify: {e}"))
                        })?;
                    Ok(vec![u8::from(true)])
                } else {
                    // C#: `return false` when the account holds no balance.
                    Ok(vec![u8::from(false)])
                }
            }
            other => Err(CoreError::invalid_operation(format!(
                "PolicyContract method '{other}' is not implemented"
            ))),
        }
    }
}
