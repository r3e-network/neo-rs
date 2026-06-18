//! PolicyContract native contract (id -7).
//!
//! Implements the C# `Neo.SmartContract.Native.PolicyContract` storage-backed
//! policy surface: fee settings, blocked accounts, whitelist fee contracts,
//! committee-gated writers, and Faun/Echidna policy extensions.

use crate::hashes::POLICY_CONTRACT_HASH;
use neo_config::Hardfork;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::{
    CallFlags, ContractParameterType, FindOptions, TransactionAttributeType, UInt160,
};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::{Interoperable, StackItem};
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;
use std::sync::LazyLock;

/// C# `PolicyContract.Prefix_FeePerByte` storage prefix.
const PREFIX_FEE_PER_BYTE: u8 = 10;
/// C# `PolicyContract.Prefix_StoragePrice` storage prefix.
const PREFIX_STORAGE_PRICE: u8 = 19;
/// C# `PolicyContract.Prefix_ExecFeeFactor` storage prefix.
const PREFIX_EXEC_FEE_FACTOR: u8 = 18;
/// C# `PolicyContract.DefaultStoragePrice`.
const DEFAULT_STORAGE_PRICE: i64 = 100_000;
/// C# `PolicyContract.Prefix_BlockedAccount` storage prefix.
const PREFIX_BLOCKED_ACCOUNT: u8 = 15;
/// C# `PolicyContract.Prefix_WhitelistedFeeContracts` storage prefix (HF_Faun).
const PREFIX_WHITELISTED_FEE_CONTRACTS: u8 = 16;
/// C# `PolicyContract.RequiredTimeForRecoverFund`: 1 year in milliseconds.
const REQUIRED_TIME_FOR_RECOVER_FUND: u64 = 365 * 24 * 60 * 60 * 1_000;
/// C# `PolicyContract.Prefix_MillisecondsPerBlock` (HF_Echidna).
const PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
/// C# `PolicyContract.Prefix_MaxValidUntilBlockIncrement` (HF_Echidna).
const PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
/// C# `PolicyContract.Prefix_MaxTraceableBlocks` (HF_Echidna).
const PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

/// Default execution fee factor (matches C# `PolicyContract.DefaultExecFeeFactor`).
pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
/// Default fee per byte (matches C# `PolicyContract.DefaultFeePerByte`).
pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
/// Default max valid-until-block increment
/// (matches C# `PolicyContract.DefaultMaxValidUntilBlockIncrement`).
pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;

/// Static accessor for the PolicyContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct PolicyContract;

impl PolicyContract {
    /// Stable native contract id (-7 in C# Policy contract).
    pub const ID: i32 = -7;
    /// Stable native contract name (matches C# `PolicyContract.Name`).
    pub const NAME: &'static str = "PolicyContract";

    /// Construct a new `PolicyContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the Policy native contract.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the script hash of the Policy native contract (static).
    pub fn script_hash() -> UInt160 {
        *POLICY_CONTRACT_HASH
    }

    fn setting_key(prefix: u8) -> StorageKey {
        StorageKey::create(PolicyContract::ID, prefix)
    }

    fn read_optional_i64_setting(
        snapshot: &DataCache,
        prefix: u8,
        setting: &str,
    ) -> CoreResult<Option<i64>> {
        match snapshot.get(&Self::setting_key(prefix)) {
            Some(item) => BigInt::from_signed_bytes_le(&item.value_bytes())
                .to_i64()
                .map(Some)
                .ok_or_else(|| {
                    CoreError::invalid_operation(format!(
                        "PolicyContract {setting} storage integer out of range"
                    ))
                }),
            None => Ok(None),
        }
    }

    fn read_required_i64_setting(
        snapshot: &DataCache,
        prefix: u8,
        setting: &str,
    ) -> CoreResult<i64> {
        Self::read_optional_i64_setting(snapshot, prefix, setting)?.ok_or_else(|| {
            CoreError::invalid_operation(format!("PolicyContract {setting} storage is missing"))
        })
    }

    fn put_required_i64_setting(
        snapshot: &DataCache,
        prefix: u8,
        setting: &str,
        value: i64,
    ) -> CoreResult<()> {
        let key = Self::setting_key(prefix);
        if snapshot.get(&key).is_none() {
            return Err(CoreError::invalid_operation(format!(
                "PolicyContract {setting} storage is missing"
            )));
        }
        snapshot.update(
            key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
        );
        Ok(())
    }

    /// Reads the max valid-until-block increment with C# `NeoSystemExtensions`
    /// semantics. Before `HF_Echidna` this is the static protocol setting; from
    /// `HF_Echidna` onward it is the Policy storage value under prefix 22, with
    /// the C# pre-genesis missing-key fallback to `ProtocolSettings`.
    pub fn get_max_valid_until_block_increment_snapshot(
        &self,
        snapshot: &neo_storage::persistence::DataCache,
        settings: &neo_config::ProtocolSettings,
    ) -> neo_error::CoreResult<u32> {
        let default = settings.max_valid_until_block_increment;
        let height = match crate::LedgerContract::new().current_index(snapshot) {
            Ok(height) => height,
            Err(err) if err.to_string().contains("current block is missing") => return Ok(default),
            Err(err) => return Err(err),
        };
        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, height) {
            return Ok(default);
        }
        let value = match Self::read_optional_i64_setting(
            snapshot,
            PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
            "MaxValidUntilBlockIncrement",
        )? {
            Some(value) => value,
            None => return Ok(default),
        };
        u32::try_from(value).map_err(|_| {
            CoreError::invalid_operation("MaxValidUntilBlockIncrement out of u32 range")
        })
    }

    /// Returns the effective `MaxTraceableBlocks` for snapshot-only callers.
    ///
    /// Mirrors C# `NeoSystemExtensions.GetMaxTraceableBlocks`: before
    /// `HF_Echidna` this is the static protocol setting; from `HF_Echidna`
    /// onward it is the Policy storage value under prefix 23, with the C#
    /// pre-genesis missing-key fallback to `ProtocolSettings`.
    pub fn get_max_traceable_blocks_snapshot(
        &self,
        snapshot: &neo_storage::persistence::DataCache,
        settings: &neo_config::ProtocolSettings,
    ) -> neo_error::CoreResult<u32> {
        let default = settings.max_traceable_blocks;
        let height = match crate::LedgerContract::new().current_index(snapshot) {
            Ok(height) => height,
            Err(err) if err.to_string().contains("current block is missing") => return Ok(default),
            Err(err) => return Err(err),
        };
        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, height) {
            return Ok(default);
        }
        let value = match Self::read_optional_i64_setting(
            snapshot,
            PREFIX_MAX_TRACEABLE_BLOCKS,
            "MaxTraceableBlocks",
        )? {
            Some(value) => value,
            None => return Ok(default),
        };
        u32::try_from(value)
            .map_err(|_| CoreError::invalid_operation("MaxTraceableBlocks out of u32 range"))
    }

    /// Reads the execution fee factor from the snapshot using C#'s
    /// `GetExecFeeFactor(settings, snapshot, index)` view. From HF_Faun onward
    /// the stored value is pico-GAS, so callers that estimate verification fees
    /// receive the legacy factor by dividing by `ApplicationEngine.FeeFactor`.
    pub fn get_exec_fee_factor_snapshot(
        &self,
        snapshot: &neo_storage::persistence::DataCache,
        settings: &neo_config::ProtocolSettings,
        height: u32,
    ) -> neo_error::CoreResult<u32> {
        let raw =
            Self::read_required_i64_setting(snapshot, PREFIX_EXEC_FEE_FACTOR, "ExecFeeFactor")?;
        let faun_enabled = settings.is_hardfork_enabled(Hardfork::HfFaun, height);
        let value = if faun_enabled { raw / FEE_FACTOR } else { raw };
        u32::try_from(value)
            .map_err(|_| CoreError::invalid_operation("ExecFeeFactor out of u32 range"))
    }

    /// Reads the fee-per-byte from the snapshot.
    pub fn get_fee_per_byte_snapshot(
        &self,
        snapshot: &neo_storage::persistence::DataCache,
    ) -> neo_error::CoreResult<u32> {
        Ok(self.fee_per_byte(snapshot)? as u32)
    }

    /// C# `GetFeePerByte` = `(long)(BigInteger)snapshot[_feePerByte]`.
    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<i64> {
        Self::read_required_i64_setting(snapshot, PREFIX_FEE_PER_BYTE, "FeePerByte")
    }

    /// C# `GetStoragePrice` = `(uint)(BigInteger)snapshot[_storagePrice]`.
    fn storage_price(&self, snapshot: &DataCache) -> CoreResult<i64> {
        Self::read_required_i64_setting(snapshot, PREFIX_STORAGE_PRICE, "StoragePrice")
    }

    /// C# `SetFeePerByte` range guard: the value must be in `[0, MAX_FEE_PER_BYTE]`.
    fn validate_fee_per_byte(value: i64) -> CoreResult<()> {
        if !(0..=MAX_FEE_PER_BYTE).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "FeePerByte must be between [0, {MAX_FEE_PER_BYTE}], got {value}"
            )));
        }
        Ok(())
    }

    /// Writes the fee-per-byte to `Prefix_FeePerByte` as a `BigInteger`, mirroring
    /// C# `GetAndChange(_feePerByte).Set(value)` (overwrite-as-Changed semantics).
    fn put_fee_per_byte(&self, snapshot: &DataCache, value: i64) -> CoreResult<()> {
        Self::put_required_i64_setting(snapshot, PREFIX_FEE_PER_BYTE, "FeePerByte", value)
    }

    /// C# `SetStoragePrice` range guard: the value must be in `[1, MAX_STORAGE_PRICE]`
    /// (C# rejects `value == 0 || value > MaxStoragePrice`).
    fn validate_storage_price(value: i64) -> CoreResult<()> {
        if !(1..=MAX_STORAGE_PRICE).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "StoragePrice must be between [1, {MAX_STORAGE_PRICE}], got {value}"
            )));
        }
        Ok(())
    }

    /// Writes the storage price to `Prefix_StoragePrice` as a `BigInteger`
    /// (C# `GetAndChange(_storagePrice).Set(value)`).
    fn put_storage_price(&self, snapshot: &DataCache, value: i64) -> CoreResult<()> {
        Self::put_required_i64_setting(snapshot, PREFIX_STORAGE_PRICE, "StoragePrice", value)
    }

    /// Reads the raw stored exec fee factor (`Prefix_ExecFeeFactor`). The value is
    /// the on-disk `BigInteger`; callers apply the HF_Faun pico-GAS scaling.
    fn exec_fee_factor_raw(&self, snapshot: &DataCache) -> CoreResult<i64> {
        Self::read_required_i64_setting(snapshot, PREFIX_EXEC_FEE_FACTOR, "ExecFeeFactor")
    }

    /// C# `SetExecFeeFactor` range guard. The upper bound is `MaxExecFeeFactor`
    /// before HF_Faun and `FeeFactor * MaxExecFeeFactor` from HF_Faun onward; the
    /// value must be at least 1 (the C# parameter is `ulong`, so a non-positive value
    /// is rejected exactly like the `value == 0` check plus the unsigned binding).
    fn validate_exec_fee_factor(&self, engine: &ApplicationEngine, value: i64) -> CoreResult<()> {
        let max_value = if engine.is_hardfork_enabled(Hardfork::HfFaun) {
            FEE_FACTOR * MAX_EXEC_FEE_FACTOR
        } else {
            MAX_EXEC_FEE_FACTOR
        };
        if !(1..=max_value).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "ExecFeeFactor must be between [1, {max_value}], got {value}"
            )));
        }
        Ok(())
    }

    /// Writes the exec fee factor to `Prefix_ExecFeeFactor` as a `BigInteger`
    /// (C# `GetAndChange(_execFeeFactor).Set(value)`).
    fn put_exec_fee_factor(&self, snapshot: &DataCache, value: i64) -> CoreResult<()> {
        Self::put_required_i64_setting(snapshot, PREFIX_EXEC_FEE_FACTOR, "ExecFeeFactor", value)
    }

    /// C# attribute-type guard shared by get/setAttributeFee: the byte must be a
    /// defined `TransactionAttributeType`, and `NotaryAssisted` is only accepted when
    /// `allow_notary_assisted` (i.e. from HF_Echidna). Mirrors
    /// `!Enum.IsDefined(...) || (!allowNotaryAssisted && type == NotaryAssisted)`.
    fn validate_attribute_type(attribute_type: u8, allow_notary_assisted: bool) -> CoreResult<()> {
        let defined = TransactionAttributeType::from_byte(attribute_type).is_some();
        let is_notary = attribute_type == TransactionAttributeType::NotaryAssisted.to_byte();
        if !defined || (!allow_notary_assisted && is_notary) {
            return Err(CoreError::invalid_operation(format!(
                "Attribute type {attribute_type} is not supported."
            )));
        }
        Ok(())
    }

    /// The `(PolicyContract.ID, [Prefix_AttributeFee, attributeType])` storage key.
    fn attribute_fee_key(attribute_type: u8) -> StorageKey {
        StorageKey::create_with_byte(PolicyContract::ID, PREFIX_ATTRIBUTE_FEE, attribute_type)
    }

    /// C# `GetAttributeFee`: validate the type, then read `Prefix_AttributeFee+type`
    /// as a `BigInteger`, defaulting to `DefaultAttributeFee` (0) when unset.
    ///
    /// Exposed `pub(crate)` so `Notary::onNEP17Payment` can read the NotaryAssisted
    /// attribute fee (C# `Policy.GetAttributeFeeV1`).
    pub(crate) fn attribute_fee(
        &self,
        snapshot: &DataCache,
        attribute_type: u8,
        allow_notary_assisted: bool,
    ) -> CoreResult<i64> {
        Self::validate_attribute_type(attribute_type, allow_notary_assisted)?;
        match snapshot.get(&Self::attribute_fee_key(attribute_type)) {
            Some(item) => BigInt::from_signed_bytes_le(&item.value_bytes())
                .to_i64()
                .ok_or_else(|| {
                    CoreError::invalid_operation("AttributeFee storage integer out of range")
                }),
            None => Ok(DEFAULT_ATTRIBUTE_FEE),
        }
    }

    /// C# `SetAttributeFee` storage effect: overwrite `Prefix_AttributeFee+type`
    /// (`GetAndChange(key, () => 0).Set(value)`).
    fn put_attribute_fee(&self, snapshot: &DataCache, attribute_type: u8, value: i64) {
        snapshot.update(
            Self::attribute_fee_key(attribute_type),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
        );
    }

    /// C# `PolicyContract.InitializeAsync(engine, hardfork)` (PolicyContract.cs:
    /// 137-170) for the NON-`ActiveIn` hardfork branches — the hardfork-scheduled
    /// re-initializations that `ContractManagement.OnPersistAsync` triggers at the
    /// hardfork's activation block:
    ///
    /// - `HF_Echidna`: seed the NotaryAssisted attribute fee (0.1 GAS per key) and
    ///   migrate `MillisecondsPerBlock` / `MaxValidUntilBlockIncrement` /
    ///   `MaxTraceableBlocks` from `ProtocolSettings` into Policy storage.
    /// - `HF_Faun`: convert the stored exec-fee factor from datoshi to pico-GAS
    ///   units (`* ApplicationEngine.FeeFactor`, faulting when Policy was never
    ///   initialized), and stamp every blocked account with the persisting block's
    ///   timestamp (the recoverFund clock).
    ///
    /// The `hardfork == ActiveIn` (genesis) branch lives in
    /// [`NativeContract::initialize`], which the persist pipeline runs at the
    /// activation block.
    pub(crate) fn initialize_for_hardfork(
        &self,
        engine: &mut ApplicationEngine,
        hardfork: Hardfork,
    ) -> CoreResult<()> {
        if hardfork == Hardfork::HfEchidna {
            let milliseconds_per_block = engine.protocol_settings().milliseconds_per_block;
            let max_valid_until_block_increment =
                engine.protocol_settings().max_valid_until_block_increment;
            let max_traceable_blocks = engine.protocol_settings().max_traceable_blocks;
            let snapshot = engine.snapshot_cache();
            snapshot.add(
                Self::attribute_fee_key(TransactionAttributeType::NotaryAssisted.to_byte()),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE,
                ))),
            );
            snapshot.add(
                StorageKey::create(PolicyContract::ID, PREFIX_MILLISECONDS_PER_BLOCK),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    milliseconds_per_block,
                ))),
            );
            snapshot.add(
                StorageKey::create(
                    PolicyContract::ID,
                    PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
                ),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    max_valid_until_block_increment,
                ))),
            );
            snapshot.add(
                StorageKey::create(PolicyContract::ID, PREFIX_MAX_TRACEABLE_BLOCKS),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    max_traceable_blocks,
                ))),
            );
        }
        if hardfork == Hardfork::HfFaun {
            // C# `GetAndChange(_execFeeFactor) ?? throw`: the factor must exist.
            let snapshot = engine.snapshot_cache();
            let factor_key = StorageKey::create(PolicyContract::ID, PREFIX_EXEC_FEE_FACTOR);
            let stored = snapshot
                .get(&factor_key)
                .ok_or_else(|| CoreError::invalid_operation("Policy was not initialized"))?;
            let factor = BigInt::from_signed_bytes_le(&stored.value_bytes())
                * neo_execution::application_engine::FEE_FACTOR;
            snapshot.update(
                factor_key,
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&factor)),
            );

            // C# stamps every blocked account with `engine.GetTime()` (the
            // persisting block's millisecond timestamp).
            let time = engine.current_block_timestamp()?;
            let stamp = crate::bigint_to_storage_bytes(&BigInt::from(time));
            for (key, _) in self.blocked_account_entries(&snapshot) {
                snapshot.update(key, StorageItem::from_bytes(stamp.clone()));
            }
        }
        Ok(())
    }

    /// The blocked-account storage key `(PolicyContract.ID, [Prefix_BlockedAccount,
    /// account])`, shared by `isBlocked` / `blockAccount` / `unblockAccount`.
    pub fn blocked_account_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(PolicyContract::ID, PREFIX_BLOCKED_ACCOUNT, account)
    }

    /// C# `PolicyContract.IsBlocked`: whether the blocked-account key exists.
    ///
    /// Exposed for transaction verification so mempool admission uses the same
    /// storage layout as the native contract rather than duplicating the prefix.
    pub fn is_blocked_snapshot(snapshot: &DataCache, account: &UInt160) -> bool {
        snapshot.get(&Self::blocked_account_key(account)).is_some()
    }

    /// Collects the `Prefix_BlockedAccount` storage entries in forward-seek order,
    /// the backing set for the `getBlockedAccounts` iterator (C# `GetBlockedAccounts`).
    fn blocked_account_entries(&self, snapshot: &DataCache) -> Vec<(StorageKey, StorageItem)> {
        let prefix_key = StorageKey::create(PolicyContract::ID, PREFIX_BLOCKED_ACCOUNT);
        snapshot
            .find(Some(&prefix_key), SeekDirection::Forward)
            .collect()
    }

    /// C# `NativeContract.IsNative(hash)`: whether `hash` is one of the canonical
    /// native-contract script hashes (`s_contractsDictionary.ContainsKey`). Used by
    /// `BlockAccountInternal` to refuse blocking a native contract.
    fn is_native_contract_hash(hash: &UInt160) -> bool {
        crate::catalog::is_standard_native_contract_hash(hash)
    }

    /// C# `PolicyContract.BlockAccountInternal` (shared by the genesis-era
    /// `blockAccount` V0 and the HF_Faun V1 — both call `AssertCommittee` first):
    /// refuse native hashes, return `false` when already blocked, clear the
    /// account's vote from HF_Faun (`NEO.VoteInternal(engine, account, null)`),
    /// then store `Prefix_BlockedAccount ++ account` with the persisting block's
    /// millisecond timestamp (`engine.GetTime()`, HF_Faun — the recoverFund
    /// request time) or empty bytes (pre-Faun).
    pub(crate) fn block_account_internal(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
    ) -> CoreResult<bool> {
        if Self::is_native_contract_hash(account) {
            return Err(CoreError::invalid_operation(
                "Cannot block a native contract.",
            ));
        }

        let key = Self::blocked_account_key(account);
        if engine.snapshot_cache().get(&key).is_some() {
            return Ok(false);
        }

        if engine.is_hardfork_enabled(Hardfork::HfFaun) {
            // C# discards VoteInternal's boolean result (false when the account has
            // no NEO state / zero balance) but propagates faults.
            let _ = crate::NeoToken::new().vote_internal(engine, account, None)?;
        }

        let value = if engine.is_hardfork_enabled(Hardfork::HfFaun) {
            // C# `new StorageItem(engine.GetTime())`: the persisting block's
            // timestamp as a BigInteger; GetTime faults without a persisting block.
            let time = engine.current_block_timestamp()?;
            crate::bigint_to_storage_bytes(&BigInt::from(time))
        } else {
            // C# `new StorageItem([])`.
            Vec::new()
        };
        engine
            .snapshot_cache()
            .update(key, StorageItem::from_bytes(value));
        Ok(true)
    }

    /// The whitelisted-fee storage key `(PolicyContract.ID,
    /// [Prefix_WhitelistedFeeContracts, contractHash, methodOffset])` — the C#
    /// `CreateStorageKey(Prefix_WhitelistedFeeContracts, contractHash,
    /// methodDescriptor.Offset)`, whose trailing `int` is big-endian (KeyBuilder
    /// `AddBigEndian(int)`).
    fn whitelist_fee_key(contract_hash: &UInt160, method_offset: i32) -> StorageKey {
        // Layout: [PREFIX, contract_hash160, method_offset_i32_be] (25 bytes).
        let mut suffix = Vec::with_capacity(20 + 4);
        suffix.extend_from_slice(&contract_hash.to_bytes());
        suffix.extend_from_slice(&method_offset.to_be_bytes());
        StorageKey::create_with_bytes(
            PolicyContract::ID,
            PREFIX_WHITELISTED_FEE_CONTRACTS,
            &suffix,
        )
    }

    /// Decodes a stored `WhitelistedContract` struct into its fields.
    fn decode_whitelisted_contract(value: &[u8]) -> CoreResult<WhitelistedContractView> {
        let limits = ExecutionEngineLimits::default();
        let decoded = BinarySerializer::deserialize_stack_value_with_limits(
            value,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("whitelisted contract: {e}")))?;
        WhitelistedContractView::from_stack_value(decoded)
    }

    /// Encodes a `WhitelistedContract` (`Struct[ContractHash, Method, ArgCount,
    /// FixedFee]`, C# `WhitelistedContract.ToStackItem`) — the write counterpart of
    /// [`decode_whitelisted_contract`].
    fn encode_whitelisted_contract(view: &WhitelistedContractView) -> CoreResult<Vec<u8>> {
        let item = view.to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::invalid_operation(format!("encode whitelisted contract: {e}")))
    }

    /// Collects the `Prefix_WhitelistedFeeContracts` storage entries in
    /// forward-seek order, the backing set for the `getWhitelistFeeContracts`
    /// iterator (C# `GetWhitelistFeeContracts`).
    fn whitelist_fee_entries(&self, snapshot: &DataCache) -> Vec<(StorageKey, StorageItem)> {
        let prefix_key = StorageKey::create(PolicyContract::ID, PREFIX_WHITELISTED_FEE_CONTRACTS);
        snapshot
            .find(Some(&prefix_key), SeekDirection::Forward)
            .collect()
    }

    /// Resolves the manifest method `(name, argCount)` of a deployed contract to
    /// its bytecode offset, the discriminant of the whitelist storage key. Mirrors
    /// the shared prologue of C# `SetWhitelistFeeContract` /
    /// `RemoveWhitelistFeeContract`: `ContractManagement.GetContract` (fault
    /// "Is not a valid contract" when missing) then
    /// `Manifest.Abi.Methods.SingleOrDefault(name, argCount)` (fault when missing
    /// or ambiguous — C# `SingleOrDefault` throws on multiple matches).
    fn resolve_whitelist_method_offset(
        &self,
        snapshot: &DataCache,
        contract_hash: &UInt160,
        method: &str,
        arg_count: i32,
    ) -> CoreResult<i32> {
        let contract =
            crate::ContractManagement::get_contract_from_snapshot(snapshot, contract_hash)?
                .ok_or_else(|| CoreError::invalid_operation("Is not a valid contract"))?;
        let arg_count = usize::try_from(arg_count).map_err(|_| {
            CoreError::invalid_operation(format!(
                "Method {method} with {arg_count} args was not found in {contract_hash}"
            ))
        })?;
        let mut matches = contract
            .manifest
            .abi
            .methods
            .iter()
            .filter(|m| m.name == method && m.parameters.len() == arg_count);
        let Some(descriptor) = matches.next() else {
            return Err(CoreError::invalid_operation(format!(
                "Method {method} with {arg_count} args was not found in {contract_hash}"
            )));
        };
        if matches.next().is_some() {
            // C# SingleOrDefault throws InvalidOperationException on >1 match.
            return Err(CoreError::invalid_operation(format!(
                "Method {method} with {arg_count} args is ambiguous in {contract_hash}"
            )));
        }
        Ok(descriptor.offset)
    }

    /// C# `NEO.GetCommittee(snapshot)`: decodes NeoToken's `Prefix_Committee`
    /// cache (an Array of `Struct[pubkey, votes]`, C#
    /// `CachedCommittee.ToStackItem`) and returns the public keys sorted ascending
    /// (`OrderBy(p => p)`). Faults when the cache is missing, matching the C#
    /// indexer throw.
    fn read_neo_committee_sorted(&self, snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
        let key = StorageKey::create(crate::NeoToken::ID, NEO_PREFIX_COMMITTEE);
        let item = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken committee cache is not initialized")
        })?;
        let limits = ExecutionEngineLimits::default();
        let decoded = BinarySerializer::deserialize_stack_value_with_limits(
            &item.value_bytes(),
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("committee cache: {e}")))?;
        let committee = crate::neo_token::CachedCommittee::from_stack_value(decoded)?;
        let mut points = committee
            .into_members()
            .into_iter()
            .map(|(point, _votes)| point)
            .collect::<Vec<_>>();
        points.sort();
        Ok(points)
    }

    /// C# `NativeContract.AssertAlmostFullCommittee`: requires a witness from the
    /// `max(max(1, n - (n - 1) / 2), n - 2)`-of-`n` multisig over the committee
    /// public keys ("signed by maximum of (half committee + 1) and
    /// (committee - 2)") and returns that multisig address. Used by `recoverFund`.
    fn assert_almost_full_committee(&self, engine: &ApplicationEngine) -> CoreResult<UInt160> {
        let snapshot = engine.snapshot_cache();
        let committees = self.read_neo_committee_sorted(&snapshot)?;
        let n = i64::try_from(committees.len())
            .map_err(|_| CoreError::invalid_operation("committee is too large"))?;
        let min = std::cmp::max(1, n - (n - 1) / 2);
        let m = std::cmp::max(min, n - 2);
        let m = usize::try_from(m)
            .map_err(|_| CoreError::invalid_operation("invalid committee threshold"))?;
        let script =
            neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
                m,
                &committees,
            )
            .map_err(|e| CoreError::invalid_operation(format!("committee multisig script: {e}")))?;
        let address = UInt160::from_script(&script);
        let authorized = engine.check_witness_hash(&address).map_err(|e| {
            CoreError::invalid_operation(format!("recoverFund committee check: {e}"))
        })?;
        if !authorized {
            return Err(CoreError::invalid_operation(
                "Invalid committee signature. It should be a multisig(max(1,len(committee) - 2))).",
            ));
        }
        Ok(address)
    }

    /// Formats the remaining wait time for `recoverFund`'s rejection message,
    /// mirroring the C# ternary chain in `PolicyContract.RecoverFund`
    /// (`{d}d {h}h {m}m` / `{h}h {m}m {s}s` / `{m}m {s}s` / `{s}s`).
    fn format_remaining_time(remaining: &BigInt) -> String {
        let zero = BigInt::from(0);
        let days = remaining / 86_400_000;
        let hours = (remaining % 86_400_000) / 3_600_000;
        let minutes = (remaining % 3_600_000) / 60_000;
        let seconds = (remaining % 60_000) / 1_000;
        if days > zero {
            format!("{days}d {hours}h {minutes}m")
        } else if hours > zero {
            format!("{hours}h {minutes}m {seconds}s")
        } else if minutes > zero {
            format!("{minutes}m {seconds}s")
        } else {
            format!("{seconds}s")
        }
    }

    /// Parses a single integer argument into an `i64` for a setter, faulting when
    /// absent or out of `i64` range (C# marshals the Integer arg to `long`/`uint`).
    fn setter_int_arg(args: &[Vec<u8>], method: &str) -> CoreResult<i64> {
        args.first()
            .map(|b| BigInt::from_signed_bytes_le(b))
            .ok_or_else(|| CoreError::invalid_operation(format!("{method} requires a value")))?
            .to_i64()
            .ok_or_else(|| CoreError::invalid_operation(format!("{method}: value out of range")))
    }

    /// Decodes the `args[index]` Hash160 parameter (C# `UInt160` marshaling: 20
    /// raw bytes, faulting otherwise).
    fn hash160_arg(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<UInt160> {
        crate::args::raw_hash160(args, index, &format!("PolicyContract::{method}"))
    }

    /// Decodes the leading `byte attributeType` argument (C# `byte` parameter
    /// binding faults for a value outside `[0, 255]`).
    fn attribute_type_arg(args: &[Vec<u8>], method: &str) -> CoreResult<u8> {
        args.first()
            .map(|b| BigInt::from_signed_bytes_le(b))
            .and_then(|b| b.to_u8())
            .ok_or_else(|| {
                CoreError::invalid_operation(format!("{method} requires a byte attribute type"))
            })
    }

    /// C# `SetMillisecondsPerBlock` range guard: `[1, MaxMillisecondsPerBlock]`.
    fn validate_milliseconds_per_block(value: i64) -> CoreResult<()> {
        if !(1..=MAX_MILLISECONDS_PER_BLOCK).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "MillisecondsPerBlock must be between [1, {MAX_MILLISECONDS_PER_BLOCK}], got {value}"
            )));
        }
        Ok(())
    }

    /// C# `GetMillisecondsPerBlock`: direct indexed read of stored
    /// `Prefix_MillisecondsPerBlock`. Shared by the getter and setter (which reads
    /// the old value for its change event).
    fn read_milliseconds_per_block(&self, engine: &ApplicationEngine) -> CoreResult<i64> {
        let snapshot = engine.snapshot_cache();
        Self::read_required_i64_setting(
            &snapshot,
            PREFIX_MILLISECONDS_PER_BLOCK,
            "MillisecondsPerBlock",
        )
    }

    /// Writes the milliseconds-per-block to `Prefix_MillisecondsPerBlock`
    /// (C# `GetAndChange(_millisecondsPerBlock).Set(value)`).
    fn put_milliseconds_per_block(&self, snapshot: &DataCache, value: i64) -> CoreResult<()> {
        Self::put_required_i64_setting(
            snapshot,
            PREFIX_MILLISECONDS_PER_BLOCK,
            "MillisecondsPerBlock",
            value,
        )
    }

    /// C# `SetMaxValidUntilBlockIncrement` range guard: `[1, 86400]`.
    fn validate_max_valid_until_block_increment(value: i64) -> CoreResult<()> {
        if !(1..=MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "MaxValidUntilBlockIncrement must be between [1, {MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT}], got {value}"
            )));
        }
        Ok(())
    }

    /// C# `SetMaxTraceableBlocks` range guard: `[1, 2102400]`.
    fn validate_max_traceable_blocks(value: i64) -> CoreResult<()> {
        if !(1..=MAX_MAX_TRACEABLE_BLOCKS).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "MaxTraceableBlocks must be between [1, {MAX_MAX_TRACEABLE_BLOCKS}], got {value}"
            )));
        }
        Ok(())
    }

    /// C# `GetMaxValidUntilBlockIncrement`: before `HF_Echidna`, callers use the
    /// protocol setting; from `HF_Echidna`, the native getter direct-indexes
    /// `Prefix_MaxValidUntilBlockIncrement`.
    ///
    /// Exposed `pub(crate)` for native Policy call paths that mirror
    /// `PolicyContract.GetMaxValidUntilBlockIncrement` from `HF_Echidna` onward.
    pub(crate) fn read_max_valid_until_block_increment(
        &self,
        engine: &ApplicationEngine,
    ) -> CoreResult<i64> {
        let default = i64::from(engine.protocol_settings().max_valid_until_block_increment);
        if !engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            return Ok(default);
        }
        let snapshot = engine.snapshot_cache();
        Self::read_required_i64_setting(
            &snapshot,
            PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
            "MaxValidUntilBlockIncrement",
        )
    }

    /// C# `IReadOnlyStore.GetMaxValidUntilBlockIncrement(ProtocolSettings)`:
    /// protocol setting before `HF_Echidna`, Policy storage after it, and the
    /// extension's pre-genesis missing-key fallback to protocol settings.
    pub(crate) fn system_max_valid_until_block_increment(
        &self,
        engine: &ApplicationEngine,
    ) -> CoreResult<i64> {
        let snapshot = engine.snapshot_cache();
        Ok(i64::from(
            self.get_max_valid_until_block_increment_snapshot(
                &snapshot,
                engine.protocol_settings(),
            )?,
        ))
    }

    /// Writes `Prefix_MaxValidUntilBlockIncrement` (C# `GetAndChange(...).Set(value)`).
    fn put_max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        value: i64,
    ) -> CoreResult<()> {
        Self::put_required_i64_setting(
            snapshot,
            PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
            "MaxValidUntilBlockIncrement",
            value,
        )
    }

    /// Writes `Prefix_MaxTraceableBlocks` (C# `GetAndChange(_maxTraceableBlocks).Set(value)`).
    fn put_max_traceable_blocks(&self, snapshot: &DataCache, value: i64) -> CoreResult<()> {
        Self::put_required_i64_setting(
            snapshot,
            PREFIX_MAX_TRACEABLE_BLOCKS,
            "MaxTraceableBlocks",
            value,
        )
    }

    /// Returns the effective `MaxTraceableBlocks` for traceability checks, mirroring
    /// the source selection in C# `LedgerContract.IsTraceableBlock`: before
    /// `HF_Echidna` it is the static `ProtocolSettings.MaxTraceableBlocks`; from
    /// `HF_Echidna` onward it is the committee-adjustable Policy value (storage
    /// prefix 23), written at activation to `ProtocolSettings.MaxTraceableBlocks`.
    ///
    /// Lives in PolicyContract because C# reads it via `Policy.GetMaxTraceableBlocks`;
    /// keeping the prefix/default here is the single source of truth shared with the
    /// `getMaxTraceableBlocks` getter.
    pub(crate) fn max_traceable_blocks(&self, engine: &ApplicationEngine) -> CoreResult<u32> {
        let default = engine.protocol_settings().max_traceable_blocks;
        if !engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            return Ok(default);
        }
        let snapshot = engine.snapshot_cache();
        let value = Self::read_required_i64_setting(
            &snapshot,
            PREFIX_MAX_TRACEABLE_BLOCKS,
            "MaxTraceableBlocks",
        )?;
        u32::try_from(value)
            .map_err(|_| CoreError::invalid_operation("MaxTraceableBlocks out of u32 range"))
    }
}

/// C# upper bound on fee-per-byte: 1 GAS in datoshi (`SetFeePerByte` rejects
/// anything outside `[0, 100000000]`).
const MAX_FEE_PER_BYTE: i64 = 100_000_000;

/// C# upper bound on storage price: `PolicyContract.MaxStoragePrice`.
const MAX_STORAGE_PRICE: i64 = 10_000_000;

/// C# `ApplicationEngine.FeeFactor` (10000): from the HF_Faun hardfork the exec
/// fee factor is stored in pico-GAS (the raw value carries this extra scaling),
/// so the legacy `getExecFeeFactor` divides it out and the bound is widened.
/// Mirrors `neo_execution::FEE_FACTOR`.
const FEE_FACTOR: i64 = 10_000;
/// C# `PolicyContract.MaxExecFeeFactor`.
const MAX_EXEC_FEE_FACTOR: i64 = 100;

/// C# `PolicyContract.Prefix_AttributeFee` storage prefix.
const PREFIX_ATTRIBUTE_FEE: u8 = 20;
/// C# `PolicyContract.DefaultAttributeFee`.
const DEFAULT_ATTRIBUTE_FEE: i64 = 0;
/// C# `PolicyContract.MaxAttributeFee` (10 GAS in datoshi).
const MAX_ATTRIBUTE_FEE: i64 = 10_0000_0000;

/// C# `PolicyContract.DefaultNotaryAssistedAttributeFee` (PolicyContract.cs:56):
/// the per-key NotaryAssisted attribute fee seeded at the HF_Echidna block.
const DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE: i64 = 1000_0000;

/// C# `NativeContract.AssertCommittee`: returns an error unless the committee
/// multisig address witnessed this call. Re-exported to
/// `crate::committee::assert_committee` for use by other native contracts.
use crate::committee::assert_committee;

/// Decoded view of a stored `WhitelistedContract` (C#
/// `Struct[ContractHash, Method, ArgCount, FixedFee]`,
/// `WhitelistedContract.FromStackItem`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct WhitelistedContractView {
    contract_hash: UInt160,
    method: String,
    arg_count: i32,
    fixed_fee: i64,
}

impl WhitelistedContractView {
    fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Struct(_, items) = stack_value else {
            return Err(CoreError::invalid_data(
                "whitelisted contract is not a struct",
            ));
        };
        let hash_bytes = items
            .first()
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract missing hash"))?
            .to_byte_string_bytes()
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract hash is not byte-like"))?;
        let contract_hash =
            crate::args::bytes_to_hash160(&hash_bytes, "whitelisted contract hash")?;
        let method = items
            .get(1)
            .and_then(neo_vm_rs::stack_value_as_string)
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract method is not UTF-8"))?;
        let arg_count = items
            .get(2)
            .and_then(neo_vm_rs::stack_value_as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract argCount out of range"))?;
        let fixed_fee = items
            .get(3)
            .and_then(neo_vm_rs::stack_value_as_i64)
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract fixedFee out of range"))?;
        Ok(Self {
            contract_hash,
            method,
            arg_count,
            fixed_fee,
        })
    }

    fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            0,
            vec![
                StackValue::ByteString(self.contract_hash.to_bytes()),
                StackValue::ByteString(self.method.as_bytes().to_vec()),
                StackValue::Integer(i64::from(self.arg_count)),
                StackValue::Integer(self.fixed_fee),
            ],
        )
    }
}

neo_vm::impl_interoperable_via_stack_value!(WhitelistedContractView);

/// C# `NeoToken.Prefix_Committee` (the committee cache NeoToken owns). Policy
/// reads it for `AssertAlmostFullCommittee`, exactly as C# Policy reaches into
/// `NativeContract.NEO.GetCommittee(engine.SnapshotCache)`.
const NEO_PREFIX_COMMITTEE: u8 = 14;

/// C# `PolicyContract.MaxMillisecondsPerBlock`.
const MAX_MILLISECONDS_PER_BLOCK: i64 = 30_000;

/// C# `PolicyContract.MaxMaxValidUntilBlockIncrement`.
const MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT: i64 = 86_400;
/// C# `PolicyContract.MaxMaxTraceableBlocks`.
const MAX_MAX_TRACEABLE_BLOCKS: i64 = 2_102_400;

static POLICY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "getFeePerByte".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "getStoragePrice".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        // Committee-gated setters: not safe, require write (States) call flags.
        NativeMethod::new(
            "setFeePerByte".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["value"]),
        NativeMethod::new(
            "setStoragePrice".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["value"]),
        // Execution fee factor: getExecFeeFactor (always present; divides out the
        // HF_Faun pico-GAS scaling), getExecPicoFeeFactor (HF_Faun; raw pico-GAS),
        // and the committee-gated setExecFeeFactor.
        NativeMethod::new(
            "getExecFeeFactor".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "getExecPicoFeeFactor".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        )
        .with_active_in(Hardfork::HfFaun),
        NativeMethod::new(
            "setExecFeeFactor".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["value"]),
        // getAttributeFee / setAttributeFee: dual C# descriptor registrations.
        // V0 is genesis-active and DeprecatedIn HF_Echidna; V1 is ActiveIn
        // HF_Echidna. The ABI signature is identical across versions, but the
        // native method cache and hardfork-gated descriptors should stay
        // literal to C#.
        NativeMethod::new(
            "getAttributeFee".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Integer],
            ContractParameterType::Integer,
        )
        .with_deprecated_in(Hardfork::HfEchidna)
        .with_parameter_names(["attributeType"]),
        NativeMethod::new(
            "getAttributeFee".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Integer],
            ContractParameterType::Integer,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["attributeType"]),
        NativeMethod::new(
            "setAttributeFee".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![
                ContractParameterType::Integer,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
        )
        .with_deprecated_in(Hardfork::HfEchidna)
        .with_parameter_names(["attributeType", "value"]),
        NativeMethod::new(
            "setAttributeFee".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![
                ContractParameterType::Integer,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["attributeType", "value"]),
        // getBlockedAccounts() -> Iterator over blocked account hashes (HF_Faun).
        NativeMethod::new(
            "getBlockedAccounts".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::InteropInterface,
        )
        .with_active_in(Hardfork::HfFaun),
        // HF_Echidna setter that emits a change notification (States|AllowNotify).
        NativeMethod::new(
            "setMillisecondsPerBlock".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["value"]),
        // HF_Echidna chain-parameter setters with cross-value invariants (States).
        NativeMethod::new(
            "setMaxValidUntilBlockIncrement".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["value"]),
        NativeMethod::new(
            "setMaxTraceableBlocks".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["value"]),
        NativeMethod::new(
            "isBlocked".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account"]),
        // Committee-gated unblock writer (not safe, States, Boolean return).
        NativeMethod::new(
            "unblockAccount".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account"]),
        // HF_Echidna moved these chain parameters from ProtocolSettings into
        // PolicyContract storage; the getters default to the settings value.
        NativeMethod::new(
            "getMillisecondsPerBlock".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        )
        .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "getMaxValidUntilBlockIncrement".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        )
        .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "getMaxTraceableBlocks".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        )
        .with_active_in(Hardfork::HfEchidna),
        // blockAccount: dual manifest registration under one name (C# V0/V1).
        // V0 = ContractMethod(true, HF_Faun): genesis-active, DeprecatedIn Faun,
        // flags States. V1 = ActiveIn HF_Faun, flags States|AllowNotify (the
        // Faun path emits NEO's Vote notification via VoteInternal). Exactly one
        // is active at any height, so the manifest/dispatcher never sees both.
        NativeMethod::new(
            "blockAccount".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        )
        .with_deprecated_in(Hardfork::HfFaun)
        .with_parameter_names(["account"]),
        NativeMethod::new(
            "blockAccount".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfFaun)
        .with_parameter_names(["account"]),
        // Whitelisted fixed-fee contracts (HF_Faun): committee-gated writers
        // that notify WhitelistFeeChanged, plus the safe iterator reader.
        NativeMethod::new(
            "setWhitelistFeeContract".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfFaun)
        .with_parameter_names(["contractHash", "method", "argCount", "fixedFee"]),
        NativeMethod::new(
            "removeWhitelistFeeContract".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
        )
        .with_active_in(Hardfork::HfFaun)
        .with_parameter_names(["contractHash", "method", "argCount"]),
        NativeMethod::new(
            "getWhitelistFeeContracts".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::InteropInterface,
        )
        .with_active_in(Hardfork::HfFaun),
        // recoverFund(account, token) -> Boolean (HF_Faun): an almost-full
        // committee sweep of a long-blocked account's NEP-17 funds to Treasury.
        NativeMethod::new(
            "recoverFund".to_string(),
            1 << 15,
            false,
            // C# v3.10.0 `PolicyContract.RecoverFund` requires only
            // States|AllowNotify; the native-to-native transfer below does not
            // add an AllowCall requirement at Policy's invocation gate.
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
            ],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfFaun)
        .with_parameter_names(["account", "token"]),
    ]
});

/// Policy's `[ContractEvent]` declarations (PolicyContract.cs:115-125), all
/// hardfork-gated: `MillisecondsPerBlockChanged` from `HF_Echidna`,
/// `WhitelistFeeChanged` and `RecoveredFund` from `HF_Faun`. (The C# names
/// come from the `*EventName` constants at PolicyContract.cs:111-113.)
static POLICY_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        NativeEvent::new(
            0,
            "MillisecondsPerBlockChanged",
            &[
                ("old", ContractParameterType::Integer),
                ("new", ContractParameterType::Integer),
            ],
        )
        .with_active_in(Hardfork::HfEchidna),
        NativeEvent::new(
            1,
            "WhitelistFeeChanged",
            &[
                ("contract", ContractParameterType::Hash160),
                ("method", ContractParameterType::String),
                ("argCount", ContractParameterType::Integer),
                ("fee", ContractParameterType::Any),
            ],
        )
        .with_active_in(Hardfork::HfFaun),
        NativeEvent::new(
            2,
            "RecoveredFund",
            &[("account", ContractParameterType::Hash160)],
        )
        .with_active_in(Hardfork::HfFaun),
    ]
});

impl NativeContract for PolicyContract {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &POLICY_METHODS
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &POLICY_EVENTS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// The `ApplicationEngine` contract-invocation gate (C#
    /// `ApplicationEngine.CallContract` -> `NativeContract.Policy.IsBlocked`):
    /// `snapshot.Contains(key(Prefix_BlockedAccount, hash))`. Native contracts can
    /// never be in the blocked list (`blockAccount` rejects them), so no special
    /// casing is needed. Without this override the trait default `Ok(false)` would
    /// let a blocked contract be invoked, diverging from C#.
    fn is_contract_blocked(
        &self,
        snapshot: &neo_storage::persistence::DataCache,
        contract_hash: &UInt160,
    ) -> CoreResult<bool> {
        Ok(snapshot
            .get(&Self::blocked_account_key(contract_hash))
            .is_some())
    }

    /// C# `PolicyContract.InitializeAsync(engine, hardfork)` for `hardfork ==
    /// ActiveIn` (PolicyContract.cs:137-143; Policy is genesis-active, so this
    /// runs while persisting block 0): seed `Prefix_FeePerByte` (1000),
    /// `Prefix_ExecFeeFactor` (30), and `Prefix_StoragePrice` (100000). The
    /// HF_Echidna / HF_Faun re-initialization branches live in
    /// [`initialize_for_hardfork`], triggered by `ContractManagement`'s
    /// `on_persist` at those hardfork blocks.
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            StorageKey::create(Self::ID, PREFIX_FEE_PER_BYTE),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_FEE_PER_BYTE,
            ))),
        );
        snapshot.add(
            StorageKey::create(Self::ID, PREFIX_EXEC_FEE_FACTOR),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_EXEC_FEE_FACTOR,
            ))),
        );
        snapshot.add(
            StorageKey::create(Self::ID, PREFIX_STORAGE_PRICE),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_STORAGE_PRICE,
            ))),
        );
        Ok(())
    }

    /// C# `PolicyContract.IsWhitelistFeeContract(snapshot, contractHash,
    /// method, out fixedFee)`, reached by the engine's contract-call fee logic
    /// through the native-contract seam: the contract must exist in
    /// ContractManagement, the `(method, paramCount)` descriptor must resolve,
    /// and a `Prefix_WhitelistedFeeContracts ++ hash ++ offset` entry must be
    /// stored — then its `FixedFee` applies instead of per-instruction fees.
    fn whitelisted_fee(
        &self,
        snapshot: &DataCache,
        contract_hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        let Some(contract) =
            crate::ContractManagement::get_contract_from_snapshot(snapshot, contract_hash)?
        else {
            return Ok(None);
        };
        let Some(descriptor) = contract
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name == method && m.parameters.len() == param_count as usize)
        else {
            return Ok(None);
        };
        match snapshot.get(&Self::whitelist_fee_key(contract_hash, descriptor.offset)) {
            Some(item) => Ok(Some(
                Self::decode_whitelisted_contract(&item.value_bytes())?.fixed_fee,
            )),
            None => Ok(None),
        }
    }

    fn invoke(
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
                let value = Self::setter_int_arg(args, "setFeePerByte")?;
                Self::validate_fee_per_byte(value)?;
                assert_committee(engine, "setFeePerByte")?;
                self.put_fee_per_byte(&engine.snapshot_cache(), value)?;
                Ok(Vec::new())
            }
            "setStoragePrice" => {
                let value = Self::setter_int_arg(args, "setStoragePrice")?;
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
                let value = Self::setter_int_arg(args, "setExecFeeFactor")?;
                self.validate_exec_fee_factor(engine, value)?;
                assert_committee(engine, "setExecFeeFactor")?;
                self.put_exec_fee_factor(&engine.snapshot_cache(), value)?;
                Ok(Vec::new())
            }
            "getAttributeFee" => {
                // C# V0/V1: allowNotaryAssisted is exactly "HF_Echidna enabled".
                let attribute_type = Self::attribute_type_arg(args, "getAttributeFee")?;
                let allow_notary = engine.is_hardfork_enabled(Hardfork::HfEchidna);
                let fee = self.attribute_fee(&snapshot, attribute_type, allow_notary)?;
                Ok(BigInt::from(fee).to_signed_bytes_le())
            }
            "setAttributeFee" => {
                // C#: validate type (NotaryAssisted gated by HF_Echidna), then
                // value <= MaxAttributeFee, then AssertCommittee, then write.
                let attribute_type = Self::attribute_type_arg(args, "setAttributeFee")?;
                let value = args
                    .get(1)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "PolicyContract::setAttributeFee requires a uint value",
                        )
                    })?;
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
                let value = Self::setter_int_arg(args, "setMillisecondsPerBlock")?;
                Self::validate_milliseconds_per_block(value)?;
                assert_committee(engine, "setMillisecondsPerBlock")?;
                let old = self.read_milliseconds_per_block(engine)?;
                self.put_milliseconds_per_block(&engine.snapshot_cache(), value)?;
                engine
                    .send_notification(
                        Self::script_hash(),
                        "MillisecondsPerBlockChanged".to_string(),
                        vec![StackItem::from_int(old), StackItem::from_int(value)],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("setMillisecondsPerBlock notify: {e}"))
                    })?;
                Ok(Vec::new())
            }
            "setMaxValidUntilBlockIncrement" => {
                // C#: range [1, 86400] -> value < MaxTraceableBlocks -> committee.
                let value = Self::setter_int_arg(args, "setMaxValidUntilBlockIncrement")?;
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
                let value = Self::setter_int_arg(args, "setMaxTraceableBlocks")?;
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
                let account = Self::hash160_arg(args, 0, "blockAccount")?;
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
                let contract_hash = Self::hash160_arg(args, 0, "setWhitelistFeeContract")?;
                let method_name = String::from_utf8(
                    args.get(1)
                        .ok_or_else(|| {
                            CoreError::invalid_operation(
                                "PolicyContract::setWhitelistFeeContract requires a method name",
                            )
                        })?
                        .clone(),
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "PolicyContract::setWhitelistFeeContract: bad method name: {e}"
                    ))
                })?;
                let arg_count = args
                    .get(2)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "PolicyContract::setWhitelistFeeContract requires an argCount",
                        )
                    })?;
                let fixed_fee = args
                    .get(3)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i64())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "PolicyContract::setWhitelistFeeContract requires a fixedFee",
                        )
                    })?;
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
                        "WhitelistFeeChanged".to_string(),
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
                let contract_hash = Self::hash160_arg(args, 0, "removeWhitelistFeeContract")?;
                let method_name = String::from_utf8(
                    args.get(1)
                        .ok_or_else(|| {
                            CoreError::invalid_operation(
                                "PolicyContract::removeWhitelistFeeContract requires a method name",
                            )
                        })?
                        .clone(),
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "PolicyContract::removeWhitelistFeeContract: bad method name: {e}"
                    ))
                })?;
                let arg_count = args
                    .get(2)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "PolicyContract::removeWhitelistFeeContract requires an argCount",
                        )
                    })?;
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
                        "WhitelistFeeChanged".to_string(),
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
                let account = Self::hash160_arg(args, 0, "recoverFund")?;
                let token = Self::hash160_arg(args, 1, "recoverFund")?;
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
                // "balanceOf", account)` — the callee runs through the VM with
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
                            "RecoveredFund".to_string(),
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

#[cfg(test)]
mod tests;
