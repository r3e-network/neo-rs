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
        StorageKey::new(PolicyContract::ID, vec![prefix])
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
        StorageKey::new(
            PolicyContract::ID,
            vec![PREFIX_ATTRIBUTE_FEE, attribute_type],
        )
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
                StorageKey::new(PolicyContract::ID, vec![PREFIX_MILLISECONDS_PER_BLOCK]),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    milliseconds_per_block,
                ))),
            );
            snapshot.add(
                StorageKey::new(
                    PolicyContract::ID,
                    vec![PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT],
                ),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    max_valid_until_block_increment,
                ))),
            );
            snapshot.add(
                StorageKey::new(PolicyContract::ID, vec![PREFIX_MAX_TRACEABLE_BLOCKS]),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    max_traceable_blocks,
                ))),
            );
        }
        if hardfork == Hardfork::HfFaun {
            // C# `GetAndChange(_execFeeFactor) ?? throw`: the factor must exist.
            let snapshot = engine.snapshot_cache();
            let factor_key = StorageKey::new(PolicyContract::ID, vec![PREFIX_EXEC_FEE_FACTOR]);
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
        StorageKey::new(
            PolicyContract::ID,
            crate::keys::prefixed_with_hash160(PREFIX_BLOCKED_ACCOUNT, account),
        )
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
        let prefix_key = StorageKey::new(PolicyContract::ID, vec![PREFIX_BLOCKED_ACCOUNT]);
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
        let mut key_bytes = vec![PREFIX_WHITELISTED_FEE_CONTRACTS];
        key_bytes.extend_from_slice(&contract_hash.to_bytes());
        key_bytes.extend_from_slice(&method_offset.to_be_bytes());
        StorageKey::new(PolicyContract::ID, key_bytes)
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
        let prefix_key =
            StorageKey::new(PolicyContract::ID, vec![PREFIX_WHITELISTED_FEE_CONTRACTS]);
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
        let key = StorageKey::new(crate::NeoToken::ID, vec![NEO_PREFIX_COMMITTEE]);
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
            StorageKey::new(Self::ID, vec![PREFIX_FEE_PER_BYTE]),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_FEE_PER_BYTE,
            ))),
        );
        snapshot.add(
            StorageKey::new(Self::ID, vec![PREFIX_EXEC_FEE_FACTOR]),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_EXEC_FEE_FACTOR,
            ))),
        );
        snapshot.add(
            StorageKey::new(Self::ID, vec![PREFIX_STORAGE_PRICE]),
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
mod tests {
    use super::*;
    use neo_primitives::UInt256;
    use neo_storage::StorageItem;

    fn seed_current_block(cache: &DataCache, index: u32) {
        let value = crate::LedgerContract::new()
            .serialize_hash_index_state(&UInt256::default(), index)
            .expect("current block pointer");
        cache.add(
            StorageKey::new(crate::LedgerContract::ID, vec![12]),
            StorageItem::from_bytes(value),
        );
    }

    fn seed_policy_setting(cache: &DataCache, prefix: u8, value: i64) {
        cache.add(
            PolicyContract::setting_key(prefix),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
        );
    }

    #[test]
    fn native_contract_surface() {
        let c = PolicyContract::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "getFeePerByte",
                "getStoragePrice",
                "setFeePerByte",
                "setStoragePrice",
                "getExecFeeFactor",
                "getExecPicoFeeFactor",
                "setExecFeeFactor",
                "getAttributeFee",
                "getAttributeFee",
                "setAttributeFee",
                "setAttributeFee",
                "getBlockedAccounts",
                "setMillisecondsPerBlock",
                "setMaxValidUntilBlockIncrement",
                "setMaxTraceableBlocks",
                "isBlocked",
                "unblockAccount",
                "getMillisecondsPerBlock",
                "getMaxValidUntilBlockIncrement",
                "getMaxTraceableBlocks",
                "blockAccount",
                "blockAccount",
                "setWhitelistFeeContract",
                "removeWhitelistFeeContract",
                "getWhitelistFeeContracts",
                "recoverFund"
            ]
        );
        // The Echidna-era chain-parameter getters are hardfork-gated.
        let mtb = c
            .methods()
            .iter()
            .find(|m| m.name == "getMaxTraceableBlocks")
            .unwrap();
        assert_eq!(mtb.active_in, Some(Hardfork::HfEchidna));
        // unblockAccount is a non-safe, write-flagged (States), Boolean writer.
        let unblock = c
            .methods()
            .iter()
            .find(|m| m.name == "unblockAccount")
            .unwrap();
        assert!(!unblock.safe);
        assert_eq!(unblock.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(unblock.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(unblock.return_type, ContractParameterType::Boolean);
        // The fee/price setters are non-safe, write-flagged (States), Void methods.
        for name in ["setFeePerByte", "setStoragePrice"] {
            let setter = c.methods().iter().find(|m| m.name == name).unwrap();
            assert!(!setter.safe, "{name} must not be safe");
            assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
            assert_eq!(setter.return_type, ContractParameterType::Void);
        }
        // The Echidna setter additionally emits a notification (States|AllowNotify).
        let ms = c
            .methods()
            .iter()
            .find(|m| m.name == "setMillisecondsPerBlock")
            .unwrap();
        assert!(!ms.safe);
        assert_eq!(
            ms.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(ms.return_type, ContractParameterType::Void);
        assert_eq!(ms.active_in, Some(Hardfork::HfEchidna));
        // The cross-validated Echidna setters are non-safe, States, Void, gated.
        for name in ["setMaxValidUntilBlockIncrement", "setMaxTraceableBlocks"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert!(!m.safe, "{name} must not be safe");
            assert_eq!(m.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(m.return_type, ContractParameterType::Void);
            assert_eq!(m.active_in, Some(Hardfork::HfEchidna));
        }
        // getExecFeeFactor is always present; getExecPicoFeeFactor is HF_Faun-gated;
        // both are safe Integer reads.
        let exec = c
            .methods()
            .iter()
            .find(|m| m.name == "getExecFeeFactor")
            .unwrap();
        assert!(exec.safe && exec.active_in.is_none());
        assert_eq!(exec.return_type, ContractParameterType::Integer);
        assert_eq!(exec.cpu_fee, 1 << 15);
        let pico = c
            .methods()
            .iter()
            .find(|m| m.name == "getExecPicoFeeFactor")
            .unwrap();
        assert!(pico.safe);
        assert_eq!(pico.active_in, Some(Hardfork::HfFaun));
        assert_eq!(pico.return_type, ContractParameterType::Integer);
        // setExecFeeFactor is a non-safe, States, Integer -> Void writer.
        let set_exec = c
            .methods()
            .iter()
            .find(|m| m.name == "setExecFeeFactor")
            .unwrap();
        assert!(!set_exec.safe);
        assert_eq!(set_exec.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(set_exec.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(set_exec.return_type, ContractParameterType::Void);
        assert!(set_exec.active_in.is_none());
        // getAttributeFee/setAttributeFee are dual C# registrations around
        // HF_Echidna. The ABI shape is unchanged, but exactly one descriptor is
        // active at a given height.
        let get_af_versions: Vec<&NativeMethod> = c
            .methods()
            .iter()
            .filter(|m| m.name == "getAttributeFee")
            .collect();
        assert_eq!(get_af_versions.len(), 2);
        for m in &get_af_versions {
            assert!(m.safe);
            assert_eq!(m.cpu_fee, 1 << 15);
            assert_eq!(m.required_call_flags, CallFlags::READ_STATES.bits());
            assert_eq!(m.parameters, vec![ContractParameterType::Integer]);
            assert_eq!(m.return_type, ContractParameterType::Integer);
        }
        assert_eq!(get_af_versions[0].deprecated_in, Some(Hardfork::HfEchidna));
        assert_eq!(get_af_versions[1].active_in, Some(Hardfork::HfEchidna));

        let set_af_versions: Vec<&NativeMethod> = c
            .methods()
            .iter()
            .filter(|m| m.name == "setAttributeFee")
            .collect();
        assert_eq!(set_af_versions.len(), 2);
        for m in &set_af_versions {
            assert!(!m.safe);
            assert_eq!(m.cpu_fee, 1 << 15);
            assert_eq!(m.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(
                m.parameters,
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer
                ]
            );
            assert_eq!(m.return_type, ContractParameterType::Void);
        }
        assert_eq!(set_af_versions[0].deprecated_in, Some(Hardfork::HfEchidna));
        assert_eq!(set_af_versions[1].active_in, Some(Hardfork::HfEchidna));
        // getBlockedAccounts is an HF_Faun-gated, safe, no-arg iterator reader.
        let blocked = c
            .methods()
            .iter()
            .find(|m| m.name == "getBlockedAccounts")
            .unwrap();
        assert_eq!(blocked.active_in, Some(Hardfork::HfFaun));
        assert!(blocked.safe && blocked.parameters.is_empty());
        assert_eq!(blocked.return_type, ContractParameterType::InteropInterface);
        assert_eq!(blocked.required_call_flags, CallFlags::READ_STATES.bits());
        // blockAccount is registered twice (C# V0/V1): V0 genesis-active and
        // DeprecatedIn HF_Faun with States; V1 ActiveIn HF_Faun with
        // States|AllowNotify. Both Hash160 -> Boolean, not safe, CpuFee 1<<15.
        let block_versions: Vec<&NativeMethod> = c
            .methods()
            .iter()
            .filter(|m| m.name == "blockAccount")
            .collect();
        assert_eq!(block_versions.len(), 2);
        for m in &block_versions {
            assert!(!m.safe);
            assert_eq!(m.cpu_fee, 1 << 15);
            assert_eq!(m.parameters, vec![ContractParameterType::Hash160]);
            assert_eq!(m.return_type, ContractParameterType::Boolean);
        }
        let v0 = block_versions
            .iter()
            .find(|m| m.deprecated_in == Some(Hardfork::HfFaun))
            .expect("blockAccount V0");
        assert_eq!(v0.active_in, None);
        assert_eq!(v0.required_call_flags, CallFlags::STATES.bits());
        let v1 = block_versions
            .iter()
            .find(|m| m.active_in == Some(Hardfork::HfFaun))
            .expect("blockAccount V1");
        assert_eq!(v1.deprecated_in, None);
        assert_eq!(
            v1.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        // Whitelist writers: HF_Faun, not safe, States|AllowNotify, Void.
        let set_wl = c
            .methods()
            .iter()
            .find(|m| m.name == "setWhitelistFeeContract")
            .unwrap();
        assert!(!set_wl.safe);
        assert_eq!(set_wl.active_in, Some(Hardfork::HfFaun));
        assert_eq!(
            set_wl.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            set_wl.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
                ContractParameterType::Integer
            ]
        );
        assert_eq!(set_wl.return_type, ContractParameterType::Void);
        let rm_wl = c
            .methods()
            .iter()
            .find(|m| m.name == "removeWhitelistFeeContract")
            .unwrap();
        assert!(!rm_wl.safe);
        assert_eq!(rm_wl.active_in, Some(Hardfork::HfFaun));
        assert_eq!(
            rm_wl.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            rm_wl.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer
            ]
        );
        assert_eq!(rm_wl.return_type, ContractParameterType::Void);
        // getWhitelistFeeContracts: HF_Faun, safe, no-arg iterator reader.
        let get_wl = c
            .methods()
            .iter()
            .find(|m| m.name == "getWhitelistFeeContracts")
            .unwrap();
        assert_eq!(get_wl.active_in, Some(Hardfork::HfFaun));
        assert!(get_wl.safe && get_wl.parameters.is_empty());
        assert_eq!(get_wl.return_type, ContractParameterType::InteropInterface);
        assert_eq!(get_wl.required_call_flags, CallFlags::READ_STATES.bits());
        // recoverFund: HF_Faun, not safe, States|AllowNotify, two Hash160 args.
        let recover = c
            .methods()
            .iter()
            .find(|m| m.name == "recoverFund")
            .unwrap();
        assert!(!recover.safe);
        assert_eq!(recover.active_in, Some(Hardfork::HfFaun));
        assert_eq!(
            recover.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            recover.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160
            ]
        );
        assert_eq!(recover.return_type, ContractParameterType::Boolean);
        assert_eq!(recover.cpu_fee, 1 << 15);
    }

    #[test]
    fn blocked_account_entries_scopes_to_prefix_blocked_account() {
        let cache = DataCache::new(false);
        // Two blocked accounts plus an unrelated fee entry that must not appear.
        let a1 = UInt160::from_bytes(&[0x11; 20]).unwrap();
        let a2 = UInt160::from_bytes(&[0x22; 20]).unwrap();
        cache.add(
            PolicyContract::blocked_account_key(&a1),
            StorageItem::from_bytes(Vec::new()),
        );
        cache.add(
            PolicyContract::blocked_account_key(&a2),
            StorageItem::from_bytes(Vec::new()),
        );
        seed_policy_setting(&cache, PREFIX_FEE_PER_BYTE, 1234); // Prefix_FeePerByte, must be excluded

        let entries = PolicyContract::new().blocked_account_entries(&cache);
        assert_eq!(entries.len(), 2);
        // Each key's suffix is [Prefix_BlockedAccount, account]; the iterator
        // strips the 1-byte prefix to yield the account hash.
        for (key, _) in &entries {
            assert_eq!(key.suffix()[0], PREFIX_BLOCKED_ACCOUNT);
            assert_eq!(key.suffix().len(), 1 + 20);
        }
    }

    #[test]
    fn attribute_fee_validates_type_and_round_trips() {
        let cache = DataCache::new(false);
        // HighPriority (0x01) is a defined type: defaults to 0, then round-trips.
        let hp = TransactionAttributeType::HighPriority.to_byte();
        assert_eq!(
            PolicyContract::new()
                .attribute_fee(&cache, hp, false)
                .unwrap(),
            DEFAULT_ATTRIBUTE_FEE
        );
        PolicyContract::new().put_attribute_fee(&cache, hp, 5_000);
        assert_eq!(
            PolicyContract::new()
                .attribute_fee(&cache, hp, false)
                .unwrap(),
            5_000
        );

        // An undefined attribute byte is rejected regardless of the notary flag.
        assert!(
            PolicyContract::new()
                .attribute_fee(&cache, 0xFE, true)
                .is_err()
        );

        // NotaryAssisted (0x22) is gated: rejected pre-Echidna (allow=false),
        // accepted from Echidna (allow=true).
        let na = TransactionAttributeType::NotaryAssisted.to_byte();
        assert!(
            PolicyContract::new()
                .attribute_fee(&cache, na, false)
                .is_err()
        );
        assert_eq!(
            PolicyContract::new()
                .attribute_fee(&cache, na, true)
                .unwrap(),
            DEFAULT_ATTRIBUTE_FEE
        );
    }

    #[test]
    fn exec_fee_factor_reads_default_and_round_trips_through_storage() {
        // Pre-Faun the reader returns the raw stored value; the writer's effect
        // is observed by the reader.
        let cache = DataCache::new(false);
        let err = PolicyContract::new()
            .exec_fee_factor_raw(&cache)
            .expect_err("missing ExecFeeFactor storage should fault");
        assert!(err.to_string().contains("ExecFeeFactor"), "{err}");

        seed_policy_setting(
            &cache,
            PREFIX_EXEC_FEE_FACTOR,
            i64::from(DEFAULT_EXEC_FEE_FACTOR),
        );
        assert_eq!(
            PolicyContract::new().exec_fee_factor_raw(&cache).unwrap(),
            i64::from(DEFAULT_EXEC_FEE_FACTOR)
        );
        PolicyContract::new()
            .put_exec_fee_factor(&cache, 50)
            .unwrap();
        assert_eq!(
            PolicyContract::new().exec_fee_factor_raw(&cache).unwrap(),
            50
        );
        // Overwrite (GetAndChange semantics).
        PolicyContract::new()
            .put_exec_fee_factor(&cache, 100)
            .unwrap();
        assert_eq!(
            PolicyContract::new().exec_fee_factor_raw(&cache).unwrap(),
            100
        );
    }

    #[test]
    fn snapshot_exec_fee_factor_divides_post_faun_pico_storage() {
        let cache = DataCache::new(false);
        let mut settings = neo_config::ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);

        seed_policy_setting(
            &cache,
            PREFIX_EXEC_FEE_FACTOR,
            i64::from(DEFAULT_EXEC_FEE_FACTOR) * FEE_FACTOR,
        );

        assert_eq!(
            PolicyContract::new()
                .get_exec_fee_factor_snapshot(&cache, &settings, 0)
                .unwrap(),
            DEFAULT_EXEC_FEE_FACTOR
        );
    }

    #[test]
    fn snapshot_max_valid_until_ignores_policy_storage_before_echidna() {
        let cache = DataCache::new(false);
        let mut settings = neo_config::ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 100);
        seed_policy_setting(&cache, PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT, 42);
        seed_current_block(&cache, 0);

        assert_eq!(
            PolicyContract::new()
                .get_max_valid_until_block_increment_snapshot(&cache, &settings)
                .unwrap(),
            settings.max_valid_until_block_increment
        );
    }

    #[test]
    fn set_fee_per_byte_validation_bounds() {
        // C# SetFeePerByte accepts [0, 100000000] and rejects outside.
        assert!(PolicyContract::validate_fee_per_byte(0).is_ok());
        assert!(PolicyContract::validate_fee_per_byte(MAX_FEE_PER_BYTE).is_ok());
        assert!(PolicyContract::validate_fee_per_byte(-1).is_err());
        assert!(PolicyContract::validate_fee_per_byte(MAX_FEE_PER_BYTE + 1).is_err());
    }

    #[test]
    fn blocked_account_key_block_then_unblock_storage_effect() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[4u8; 20]).unwrap();
        let key = PolicyContract::blocked_account_key(&account);
        // Not blocked initially.
        assert!(cache.get(&key).is_none());
        // Block (add) then unblock (delete) — the exact storage effect the
        // isBlocked / unblockAccount arms rely on.
        cache.add(key.clone(), StorageItem::from_bytes(vec![]));
        assert!(cache.get(&key).is_some());
        cache.delete(&key);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn fee_per_byte_write_then_read_round_trips() {
        let cache = DataCache::new(false);
        // Writing via the setter's storage effect is observed by the getter,
        // exercising the GetAndChange (overwrite-as-Changed) semantics.
        seed_policy_setting(&cache, PREFIX_FEE_PER_BYTE, i64::from(DEFAULT_FEE_PER_BYTE));
        PolicyContract::new()
            .put_fee_per_byte(&cache, 4242)
            .unwrap();
        assert_eq!(PolicyContract::new().fee_per_byte(&cache).unwrap(), 4242);
        // Overwriting an existing value is read back as the new value.
        PolicyContract::new()
            .put_fee_per_byte(&cache, 5000)
            .unwrap();
        assert_eq!(PolicyContract::new().fee_per_byte(&cache).unwrap(), 5000);
    }

    #[test]
    fn set_storage_price_validation_bounds() {
        // C# SetStoragePrice accepts [1, MaxStoragePrice] and rejects outside.
        assert!(PolicyContract::validate_storage_price(1).is_ok());
        assert!(PolicyContract::validate_storage_price(MAX_STORAGE_PRICE).is_ok());
        assert!(PolicyContract::validate_storage_price(0).is_err());
        assert!(PolicyContract::validate_storage_price(MAX_STORAGE_PRICE + 1).is_err());
    }

    #[test]
    fn storage_price_write_then_read_round_trips() {
        let cache = DataCache::new(false);
        seed_policy_setting(&cache, PREFIX_STORAGE_PRICE, DEFAULT_STORAGE_PRICE);
        PolicyContract::new()
            .put_storage_price(&cache, 250_000)
            .unwrap();
        assert_eq!(
            PolicyContract::new().storage_price(&cache).unwrap(),
            250_000
        );
        PolicyContract::new()
            .put_storage_price(&cache, 1_000_000)
            .unwrap();
        assert_eq!(
            PolicyContract::new().storage_price(&cache).unwrap(),
            1_000_000
        );
    }

    #[test]
    fn set_milliseconds_per_block_validation_bounds() {
        // C# SetMillisecondsPerBlock accepts [1, MaxMillisecondsPerBlock].
        assert!(PolicyContract::validate_milliseconds_per_block(1).is_ok());
        assert!(
            PolicyContract::validate_milliseconds_per_block(MAX_MILLISECONDS_PER_BLOCK).is_ok()
        );
        assert!(PolicyContract::validate_milliseconds_per_block(0).is_err());
        assert!(
            PolicyContract::validate_milliseconds_per_block(MAX_MILLISECONDS_PER_BLOCK + 1)
                .is_err()
        );
    }

    #[test]
    fn milliseconds_per_block_write_persists_to_storage() {
        let cache = DataCache::new(false);
        seed_policy_setting(&cache, PREFIX_MILLISECONDS_PER_BLOCK, 15_000);
        PolicyContract::new()
            .put_milliseconds_per_block(&cache, 7_000)
            .unwrap();
        // Read back the raw storage value (the engine-aware getter adds the
        // ProtocolSettings default, which isn't needed once a value is stored).
        assert_eq!(
            crate::read_storage_int(&cache, PolicyContract::ID, PREFIX_MILLISECONDS_PER_BLOCK, 0)
                .unwrap(),
            7_000
        );
    }

    #[test]
    fn max_chain_param_setter_range_bounds() {
        // C# MaxMaxValidUntilBlockIncrement = 86400, MaxMaxTraceableBlocks = 2102400.
        assert!(PolicyContract::validate_max_valid_until_block_increment(1).is_ok());
        assert!(
            PolicyContract::validate_max_valid_until_block_increment(
                MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT
            )
            .is_ok()
        );
        assert!(PolicyContract::validate_max_valid_until_block_increment(0).is_err());
        assert!(
            PolicyContract::validate_max_valid_until_block_increment(
                MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT + 1
            )
            .is_err()
        );

        assert!(PolicyContract::validate_max_traceable_blocks(1).is_ok());
        assert!(PolicyContract::validate_max_traceable_blocks(MAX_MAX_TRACEABLE_BLOCKS).is_ok());
        assert!(PolicyContract::validate_max_traceable_blocks(0).is_err());
        assert!(
            PolicyContract::validate_max_traceable_blocks(MAX_MAX_TRACEABLE_BLOCKS + 1).is_err()
        );
    }

    #[test]
    fn max_chain_param_writes_persist_to_storage() {
        let cache = DataCache::new(false);
        seed_policy_setting(&cache, PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT, 5_760);
        PolicyContract::new()
            .put_max_valid_until_block_increment(&cache, 5_000)
            .unwrap();
        assert_eq!(
            crate::read_storage_int(
                &cache,
                PolicyContract::ID,
                PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
                0
            )
            .unwrap(),
            5_000
        );
        seed_policy_setting(&cache, PREFIX_MAX_TRACEABLE_BLOCKS, 2_102_400);
        PolicyContract::new()
            .put_max_traceable_blocks(&cache, 1_000_000)
            .unwrap();
        assert_eq!(
            crate::read_storage_int(&cache, PolicyContract::ID, PREFIX_MAX_TRACEABLE_BLOCKS, 0)
                .unwrap(),
            1_000_000
        );
    }

    #[test]
    fn is_blocked_checks_storage_existence() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[3u8; 20]).unwrap();
        let key = {
            let mut k = vec![PREFIX_BLOCKED_ACCOUNT];
            k.extend_from_slice(&account.to_bytes());
            StorageKey::new(PolicyContract::ID, k)
        };
        // Not blocked until a record exists.
        assert!(cache.get(&key).is_none());
        cache.add(key.clone(), StorageItem::from_bytes(vec![]));
        assert!(cache.get(&key).is_some());
    }

    #[test]
    fn is_contract_blocked_trait_reflects_blocked_list() {
        // Regression: the engine's contract-invocation gate (contracts.rs) calls
        // the NativeContract::is_contract_blocked TRAIT method. It must reflect
        // the blocked-account list rather than the default Ok(false) — otherwise
        // a blocked contract could be invoked, diverging from C#.
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
        let policy = PolicyContract::new();
        assert!(
            !<PolicyContract as NativeContract>::is_contract_blocked(&policy, &cache, &hash)
                .unwrap()
        );
        cache.add(
            PolicyContract::blocked_account_key(&hash),
            StorageItem::from_bytes(vec![]),
        );
        assert!(
            <PolicyContract as NativeContract>::is_contract_blocked(&policy, &cache, &hash)
                .unwrap()
        );
    }

    #[test]
    fn fee_per_byte_reads_storage_with_default() {
        let cache = DataCache::new(false);
        let err = PolicyContract::new()
            .fee_per_byte(&cache)
            .expect_err("missing FeePerByte storage should fault");
        assert!(err.to_string().contains("FeePerByte"), "{err}");

        // A configured value is read back from the BigInteger storage item.
        seed_policy_setting(&cache, PREFIX_FEE_PER_BYTE, 4242);
        assert_eq!(PolicyContract::new().fee_per_byte(&cache).unwrap(), 4242);
    }

    #[test]
    fn storage_price_reads_storage_with_default() {
        let cache = DataCache::new(false);
        let err = PolicyContract::new()
            .storage_price(&cache)
            .expect_err("missing StoragePrice storage should fault");
        assert!(err.to_string().contains("StoragePrice"), "{err}");

        seed_policy_setting(&cache, PREFIX_STORAGE_PRICE, 250_000);
        assert_eq!(
            PolicyContract::new().storage_price(&cache).unwrap(),
            250_000
        );
    }

    #[test]
    fn echidna_policy_settings_require_initialized_storage() {
        let cache = DataCache::new(false);
        seed_current_block(&cache, 0);
        let mut settings = neo_config::ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 0);
        let engine = ApplicationEngine::new(
            neo_primitives::TriggerType::Application,
            None,
            std::sync::Arc::new(cache),
            None,
            settings.clone(),
            0,
            None,
        )
        .expect("engine builds");

        for (name, result) in [
            (
                "MillisecondsPerBlock",
                PolicyContract::new().read_milliseconds_per_block(&engine),
            ),
            (
                "MaxValidUntilBlockIncrement",
                PolicyContract::new().read_max_valid_until_block_increment(&engine),
            ),
            (
                "MaxTraceableBlocks",
                PolicyContract::new()
                    .max_traceable_blocks(&engine)
                    .map(i64::from),
            ),
        ] {
            let err = result.expect_err("missing Echidna policy storage should fault");
            assert!(err.to_string().contains(name), "{err}");
        }

        let snapshot = engine.snapshot_cache();
        assert_eq!(
            PolicyContract::new()
                .get_max_valid_until_block_increment_snapshot(&snapshot, &settings)
                .unwrap(),
            settings.max_valid_until_block_increment
        );
        assert_eq!(
            PolicyContract::new()
                .get_max_traceable_blocks_snapshot(&snapshot, &settings)
                .unwrap(),
            settings.max_traceable_blocks
        );
    }

    #[test]
    fn whitelisted_contract_struct_round_trips() {
        // C# WhitelistedContract.ToStackItem/FromStackItem: a Struct of
        // [ContractHash, Method, ArgCount, FixedFee].
        let view = WhitelistedContractView {
            contract_hash: UInt160::from_bytes(&[0x42; 20]).unwrap(),
            method: "balanceOf".to_string(),
            arg_count: 1,
            fixed_fee: 123_456,
        };
        let bytes = PolicyContract::encode_whitelisted_contract(&view).unwrap();
        let expected_item = StackItem::from_struct(vec![
            StackItem::from_byte_string(view.contract_hash.to_bytes()),
            StackItem::from_byte_string(view.method.as_bytes().to_vec()),
            StackItem::from_int(BigInt::from(view.arg_count)),
            StackItem::from_int(BigInt::from(view.fixed_fee)),
        ]);
        let expected =
            BinarySerializer::serialize(&expected_item, &ExecutionEngineLimits::default()).unwrap();
        assert_eq!(bytes, expected);
        let decoded = PolicyContract::decode_whitelisted_contract(&bytes).unwrap();
        assert_eq!(decoded, view);
        assert_eq!(
            Interoperable::to_stack_value(&view).unwrap(),
            StackValue::try_from(expected_item.clone()).unwrap()
        );

        let mut trait_decoded = WhitelistedContractView {
            contract_hash: UInt160::from_bytes(&[0x00; 20]).unwrap(),
            method: String::new(),
            arg_count: 0,
            fixed_fee: 0,
        };
        Interoperable::from_stack_value(
            &mut trait_decoded,
            StackValue::try_from(expected_item).unwrap(),
        )
        .unwrap();
        assert_eq!(trait_decoded, view);
    }

    #[test]
    fn whitelisted_contract_storage_uses_stack_value_projection() {
        fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
            let start_index = source.find(start).expect("start marker exists");
            let end_index = source[start_index..]
                .find(end)
                .map(|offset| start_index + offset)
                .expect("end marker exists");
            &source[start_index..end_index]
        }

        let source = include_str!("policy_contract.rs");
        let decoder = slice_between(
            source,
            "fn decode_whitelisted_contract",
            "fn encode_whitelisted_contract",
        );
        assert!(decoder.contains("deserialize_stack_value_with_limits"));
        assert!(decoder.contains("WhitelistedContractView::from_stack_value"));
        assert!(!decoder.contains("BinarySerializer::deserialize("));

        let encoder = slice_between(
            source,
            "fn encode_whitelisted_contract",
            "fn whitelist_fee_entries",
        );
        assert!(encoder.contains("to_stack_value"));
        assert!(encoder.contains("serialize_stack_value_default"));
        assert!(!encoder.contains("StackItem::from_struct"));
        assert!(!encoder.contains("BinarySerializer::serialize("));
    }

    #[test]
    fn committee_cache_reader_uses_stack_value_projection() {
        let source = include_str!("policy_contract.rs");
        let start = source
            .find("fn read_neo_committee_sorted")
            .expect("committee reader exists");
        let end = source[start..]
            .find("fn assert_almost_full_committee")
            .map(|offset| start + offset)
            .expect("assert_almost_full_committee follows committee reader");
        let helper = &source[start..end];

        assert!(helper.contains("deserialize_stack_value_with_limits"));
        assert!(helper.contains("CachedCommittee::from_stack_value"));
        assert!(!helper.contains("StackValue::Array"));
        assert!(!helper.contains("StackValue::Struct"));
        assert!(!helper.contains("BinarySerializer::deserialize("));
        assert!(!helper.contains("StackItem::Array"));
        assert!(!helper.contains("StackItem::Struct"));
    }

    #[test]
    fn whitelist_fee_key_is_prefix_hash_and_big_endian_offset() {
        // C# CreateStorageKey(Prefix_WhitelistedFeeContracts, contractHash,
        // methodDescriptor.Offset): [16] ++ hash(20) ++ offset as big-endian i32.
        let hash = UInt160::from_bytes(&[0xAB; 20]).unwrap();
        let key = PolicyContract::whitelist_fee_key(&hash, 0x0102_0304);
        let suffix = key.suffix();
        assert_eq!(suffix.len(), 1 + 20 + 4);
        assert_eq!(suffix[0], PREFIX_WHITELISTED_FEE_CONTRACTS);
        assert_eq!(&suffix[1..21], &[0xAB; 20]);
        assert_eq!(&suffix[21..25], &[0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn whitelist_fee_entries_scope_to_prefix() {
        let cache = DataCache::new(false);
        let h1 = UInt160::from_bytes(&[0x11; 20]).unwrap();
        let h2 = UInt160::from_bytes(&[0x22; 20]).unwrap();
        let entry = |hash: &UInt160, method: &str| {
            PolicyContract::encode_whitelisted_contract(&WhitelistedContractView {
                contract_hash: *hash,
                method: method.to_string(),
                arg_count: 0,
                fixed_fee: 5,
            })
            .unwrap()
        };
        cache.add(
            PolicyContract::whitelist_fee_key(&h1, 0),
            StorageItem::from_bytes(entry(&h1, "a")),
        );
        cache.add(
            PolicyContract::whitelist_fee_key(&h2, 7),
            StorageItem::from_bytes(entry(&h2, "b")),
        );
        // An unrelated blocked-account record must not appear.
        cache.add(
            PolicyContract::blocked_account_key(&h1),
            StorageItem::from_bytes(Vec::new()),
        );

        let entries = PolicyContract::new().whitelist_fee_entries(&cache);
        assert_eq!(entries.len(), 2);
        for (key, _) in &entries {
            assert_eq!(key.suffix()[0], PREFIX_WHITELISTED_FEE_CONTRACTS);
            assert_eq!(key.suffix().len(), 1 + 20 + 4);
        }
    }

    #[test]
    fn native_hashes_cannot_be_blocked() {
        // C# BlockAccountInternal: IsNative(account) -> fault. All 11 canonical
        // native hashes must be covered; a regular account must not.
        for spec in crate::standard_native_contract_specs() {
            assert!(
                PolicyContract::is_native_contract_hash(&spec.hash),
                "{} ({}) is native",
                spec.name,
                spec.hash
            );
        }
        assert!(!PolicyContract::is_native_contract_hash(
            &UInt160::from_bytes(&[0x42; 20]).unwrap()
        ));
    }

    #[test]
    fn remaining_time_message_matches_csharp_format() {
        // C# RecoverFund's ternary chain: days -> "{d}d {h}h {m}m",
        // hours -> "{h}h {m}m {s}s", minutes -> "{m}m {s}s", else "{s}s".
        let ms = |d: i64, h: i64, m: i64, s: i64| {
            d * 86_400_000 + h * 3_600_000 + m * 60_000 + s * 1_000
        };
        assert_eq!(
            PolicyContract::format_remaining_time(&BigInt::from(ms(2, 3, 4, 5))),
            "2d 3h 4m"
        );
        assert_eq!(
            PolicyContract::format_remaining_time(&BigInt::from(ms(0, 3, 4, 5))),
            "3h 4m 5s"
        );
        assert_eq!(
            PolicyContract::format_remaining_time(&BigInt::from(ms(0, 0, 4, 5))),
            "4m 5s"
        );
        assert_eq!(
            PolicyContract::format_remaining_time(&BigInt::from(ms(0, 0, 0, 5))),
            "5s"
        );
        assert_eq!(
            PolicyContract::format_remaining_time(&BigInt::from(999)),
            "0s"
        );
    }

    #[test]
    fn required_recover_fund_time_is_one_year_of_milliseconds() {
        // C# RequiredTimeForRecoverFund = 365 * 24 * 60 * 60 * 1_000UL.
        assert_eq!(REQUIRED_TIME_FOR_RECOVER_FUND, 31_536_000_000);
    }
}

/// End-to-end verification of the committee-gated PolicyContract writers
/// through the VM (the witness-gated script-execution path proven by
/// `neo_token::witness_harness_tests`): a script `System.Contract.Call`s
/// PolicyContract with the committee multisig address as signer, and the
/// resulting storage transitions are asserted against the shared snapshot.
#[cfg(test)]
mod policy_writer_tests {
    use super::*;
    use crate::test_support::{
        CM_PREFIX_CONTRACT, NEO_PREFIX_COMMITTEE, POLICY_PREFIX_ATTRIBUTE_FEE, committee_address,
        deploy_native, hex, sample_committee, seed_committee,
    };
    use neo_config::ProtocolSettings;
    use neo_execution::contract_state::ContractState;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_manifest::{ContractManifest, ContractMethodDescriptor, NefFile};
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_payloads::{Block, BlockHeader};
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    /// ProtocolSettings with HF_Faun scheduled from genesis.
    fn faun_settings() -> ProtocolSettings {
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        settings
    }

    /// Runs `method(args...)` on PolicyContract via System.Contract.Call,
    /// signed (Global) by `signer`, against the shared `snapshot`. The closure
    /// must push the call arguments in REVERSE order (deepest first). Returns
    /// the final VM state and the finished engine (for result-stack and
    /// notification assertions).
    fn call_policy_engine(
        snapshot: Arc<DataCache>,
        signer: UInt160,
        settings: ProtocolSettings,
        block: Option<Block>,
        method: &str,
        argc: i64,
        push_args_reversed: &dyn Fn(&mut ScriptBuilder),
    ) -> (VmState, ApplicationEngine) {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut builder = ScriptBuilder::new();
        push_args_reversed(&mut builder);
        builder.emit_push_int(argc);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push(method.as_bytes());
        builder.emit_push(&PolicyContract::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            block,
            settings,
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        (state, engine)
    }

    /// [`call_policy_engine`] reduced to the final VM state and the boolean on
    /// top of the result stack (if any).
    fn call_policy(
        snapshot: Arc<DataCache>,
        signer: UInt160,
        settings: ProtocolSettings,
        block: Option<Block>,
        method: &str,
        argc: i64,
        push_args_reversed: &dyn Fn(&mut ScriptBuilder),
    ) -> (VmState, Option<bool>) {
        let (state, engine) = call_policy_engine(
            snapshot,
            signer,
            settings,
            block,
            method,
            argc,
            push_args_reversed,
        );
        let top = engine
            .result_stack()
            .peek(0)
            .ok()
            .and_then(|item| item.as_bool().ok());
        (state, top)
    }

    fn returning_user_contract(hash: UInt160) -> ContractState {
        let nef = NefFile::new(
            "policy-blocked-call-test".to_string(),
            vec![
                neo_vm_rs::OpCode::PUSH1.byte(),
                neo_vm_rs::OpCode::RET.byte(),
            ],
        );
        let mut manifest = ContractManifest::new("BlockedCallFixture".to_string());
        manifest.abi.methods.push(
            ContractMethodDescriptor::new(
                "answer".to_string(),
                Vec::new(),
                ContractParameterType::Integer,
                0,
                true,
            )
            .expect("method descriptor"),
        );
        ContractState::new(7, hash, nef, manifest)
    }

    #[test]
    fn real_policy_blocked_storage_rejects_system_contract_call_target() {
        crate::install();
        let settings = ProtocolSettings::default();
        let cache = DataCache::new(false);
        let target_hash = UInt160::parse("0xb1b2c3d4e5f60718293a4b5c6d7e8f0102030405").unwrap();
        deploy_native(&cache, &returning_user_contract(target_hash));
        cache.add(
            PolicyContract::blocked_account_key(&target_hash),
            StorageItem::from_bytes(Vec::new()),
        );

        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("answer".as_bytes());
        builder.emit_push(&target_hash.to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::new(cache),
            None,
            settings,
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");

        let state = engine.execute_allow_fault();
        assert_eq!(
            state,
            VmState::FAULT,
            "C# ApplicationEngine.CallContractInternal rejects Policy-blocked contracts before invocation"
        );
        assert_eq!(
            engine.invocation_stack().len(),
            1,
            "blocked contract target must not be loaded as an invocation context"
        );
    }

    /// Pre-Faun blockAccount (the V0 registration): committee-gated, writes an
    /// empty `Prefix_BlockedAccount` record, and double-blocking returns false
    /// (C# UT_PolicyContract.Check_BlockAccount).
    #[test]
    fn block_account_e2e_pre_faun_blocks_then_double_block_returns_false() {
        crate::install();
        // Default MainNet schedules Faun at 8,800,000, so block 0 is pre-Faun.
        let settings = ProtocolSettings::default();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        let snapshot = Arc::new(cache);
        let signer = committee_address(&committee);
        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();

        let (state, result) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings.clone(),
            None,
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(state, VmState::HALT, "blockAccount must HALT");
        assert_eq!(result, Some(true), "first block returns true");
        let item = snapshot
            .get(&PolicyContract::blocked_account_key(&account))
            .expect("blocked entry written");
        assert!(
            item.value_bytes().is_empty(),
            "pre-Faun blocked value is empty"
        );

        // Blocking the same account again returns false (no fault).
        let (state2, result2) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings,
            None,
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(state2, VmState::HALT, "double block must still HALT");
        assert_eq!(result2, Some(false), "double block returns false");
    }

    /// blockAccount without the committee witness faults (C# AssertCommittee
    /// throws) and writes nothing.
    #[test]
    fn block_account_e2e_requires_committee_witness() {
        crate::install();
        let settings = ProtocolSettings::default();
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        let snapshot = Arc::new(cache);
        let stranger = UInt160::from_bytes(&[0x09; 20]).unwrap();
        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();

        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            stranger,
            settings,
            None,
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(
            state,
            VmState::FAULT,
            "non-committee blockAccount must FAULT"
        );
        assert!(
            snapshot
                .get(&PolicyContract::blocked_account_key(&account))
                .is_none()
        );
    }

    /// blockAccount on a native contract hash faults ("Cannot block a native
    /// contract.") even with the committee witness.
    #[test]
    fn block_account_e2e_rejects_native_contract_hash() {
        crate::install();
        let settings = ProtocolSettings::default();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        let snapshot = Arc::new(cache);
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;

        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            None,
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&gas_hash.to_array());
            },
        );
        assert_eq!(state, VmState::FAULT, "blocking a native hash must FAULT");
        assert!(
            snapshot
                .get(&PolicyContract::blocked_account_key(&gas_hash))
                .is_none()
        );
    }

    /// Faun-path blockAccount (the V1 registration): clears the account's vote
    /// via NEO.VoteInternal (candidate weight drops, VoteTo cleared,
    /// _votersCount reduced) and stamps the blocked entry with the persisting
    /// block's millisecond timestamp (`engine.GetTime()`).
    #[test]
    fn block_account_e2e_faun_clears_vote_and_stamps_time() {
        const BLOCK_TIME_MS: u64 = 1_234_567_890;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );

        // A registered candidate with 100 votes, all from `voter` (balance 100,
        // voting since height 0), and the matching _votersCount.
        let candidate = committee[0].clone();
        let voter = UInt160::from_bytes(&[0x07; 20]).unwrap();
        let candidate_state =
            StackItem::from_struct(vec![StackItem::from_bool(true), StackItem::from_int(100)]);
        let mut candidate_key = vec![33u8]; // NeoToken Prefix_Candidate
        candidate_key.extend_from_slice(&candidate.to_bytes());
        let candidate_key = StorageKey::new(crate::NeoToken::ID, candidate_key);
        cache.add(
            candidate_key.clone(),
            StorageItem::from_bytes(
                BinarySerializer::serialize(&candidate_state, &ExecutionEngineLimits::default())
                    .unwrap(),
            ),
        );
        let voter_state = StackItem::from_struct(vec![
            StackItem::from_int(100),                          // Balance
            StackItem::from_int(0),                            // BalanceHeight
            StackItem::from_byte_string(candidate.to_bytes()), // VoteTo
            StackItem::from_int(0),                            // LastGasPerVote
        ]);
        let mut voter_key = vec![20u8]; // NEP-17 Prefix_Account
        voter_key.extend_from_slice(&voter.to_bytes());
        let voter_key = StorageKey::new(crate::NeoToken::ID, voter_key);
        cache.add(
            voter_key.clone(),
            StorageItem::from_bytes(
                BinarySerializer::serialize(&voter_state, &ExecutionEngineLimits::default())
                    .unwrap(),
            ),
        );
        let voters_count_key = StorageKey::new(crate::NeoToken::ID, vec![1u8]);
        cache.add(
            voters_count_key.clone(),
            StorageItem::from_bytes(BigInt::from(100).to_signed_bytes_le()),
        );
        let snapshot = Arc::new(cache);

        // Persisting block at index 100 with a known timestamp (GetTime source).
        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(BLOCK_TIME_MS);
        let block = Block::from_parts(header, vec![]);

        let (state, result) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(block),
            "blockAccount",
            1,
            &|b| {
                b.emit_push(&voter.to_array());
            },
        );
        assert_eq!(state, VmState::HALT, "Faun blockAccount must HALT");
        assert_eq!(result, Some(true));

        // The blocked entry carries the block timestamp (the recoverFund clock).
        let blocked = snapshot
            .get(&PolicyContract::blocked_account_key(&voter))
            .expect("blocked entry written");
        assert_eq!(
            blocked.value_bytes().into_owned(),
            BigInt::from(BLOCK_TIME_MS).to_signed_bytes_le()
        );

        // The candidate lost the voter's 100-NEO weight (still registered).
        let cand = snapshot.get(&candidate_key).expect("candidate entry kept");
        let decoded = BinarySerializer::deserialize(
            &cand.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("candidate state is not a struct");
        };
        assert!(
            fields.items()[0].as_bool().unwrap(),
            "candidate stays registered"
        );
        assert_eq!(
            fields.items()[1].as_int().unwrap(),
            BigInt::from(0),
            "votes cleared"
        );

        // The voter's VoteTo is now null and the reward markers advanced.
        let acct = snapshot.get(&voter_key).expect("voter account kept");
        let decoded = BinarySerializer::deserialize(
            &acct.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("voter account state is not a struct");
        };
        assert_eq!(
            fields.items()[0].as_int().unwrap(),
            BigInt::from(100),
            "balance kept"
        );
        assert!(
            matches!(fields.items()[2], StackItem::Null),
            "VoteTo cleared"
        );

        // _votersCount dropped by the voter's balance (100 -> 0).
        let voters = snapshot.get(&voters_count_key).expect("voters count kept");
        assert_eq!(
            BigInt::from_signed_bytes_le(&voters.value_bytes()),
            BigInt::from(0)
        );
    }

    /// setWhitelistFeeContract / removeWhitelistFeeContract round trip (HF_Faun):
    /// the committee whitelists NEO.balanceOf (mirroring C# TestWhiteListFee),
    /// the entry lands under [16] ++ hash ++ offset(BE) with the
    /// WhitelistedContract struct value, the `whitelisted_fee` seam reads it
    /// back, and the remove writer deletes it again.
    #[test]
    fn whitelist_fee_contract_e2e_set_then_remove() {
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        // The whitelist target: NEO's deployed state (its manifest carries the
        // balanceOf(1) descriptor whose offset keys the whitelist entry).
        let neo_state = build_native_contract_state(&crate::NeoToken, &settings, 0);
        let balance_of_offset = neo_state
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name == "balanceOf" && m.parameters.len() == 1)
            .expect("NEO balanceOf in manifest")
            .offset;
        deploy_native(&cache, &neo_state);
        let snapshot = Arc::new(cache);
        let signer = committee_address(&committee);
        let neo_hash = crate::NeoToken::script_hash();

        // setWhitelistFeeContract(NEO, "balanceOf", 1, 12345).
        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings.clone(),
            None,
            "setWhitelistFeeContract",
            4,
            &|b| {
                b.emit_push_int(12345); // fixedFee (arg 3, deepest)
                b.emit_push_int(1); // argCount (arg 2)
                b.emit_push("balanceOf".as_bytes()); // method (arg 1)
                b.emit_push(&neo_hash.to_array()); // contractHash (arg 0, top)
            },
        );
        assert_eq!(state, VmState::HALT, "setWhitelistFeeContract must HALT");
        let key = PolicyContract::whitelist_fee_key(&neo_hash, balance_of_offset);
        let item = snapshot.get(&key).expect("whitelist entry written");
        let view = PolicyContract::decode_whitelisted_contract(&item.value_bytes()).unwrap();
        assert_eq!(view.contract_hash, neo_hash);
        assert_eq!(view.method, "balanceOf");
        assert_eq!(view.arg_count, 1);
        assert_eq!(view.fixed_fee, 12345);

        // The engine-facing seam (C# IsWhitelistFeeContract) resolves the fee.
        assert_eq!(
            NativeContract::whitelisted_fee(
                &PolicyContract::new(),
                &snapshot,
                &neo_hash,
                "balanceOf",
                1
            )
            .unwrap(),
            Some(12345)
        );
        // A different method / a missing contract resolve to no whitelist.
        assert_eq!(
            NativeContract::whitelisted_fee(
                &PolicyContract::new(),
                &snapshot,
                &neo_hash,
                "transfer",
                4
            )
            .unwrap(),
            None
        );
        let unknown = UInt160::from_bytes(&[0x55; 20]).unwrap();
        assert_eq!(
            NativeContract::whitelisted_fee(
                &PolicyContract::new(),
                &snapshot,
                &unknown,
                "balanceOf",
                1
            )
            .unwrap(),
            None
        );

        // removeWhitelistFeeContract(NEO, "balanceOf", 1) deletes the entry.
        let (state2, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings.clone(),
            None,
            "removeWhitelistFeeContract",
            3,
            &|b| {
                b.emit_push_int(1); // argCount (arg 2, deepest)
                b.emit_push("balanceOf".as_bytes()); // method (arg 1)
                b.emit_push(&neo_hash.to_array()); // contractHash (arg 0, top)
            },
        );
        assert_eq!(
            state2,
            VmState::HALT,
            "removeWhitelistFeeContract must HALT"
        );
        assert!(snapshot.get(&key).is_none(), "whitelist entry deleted");

        // Removing again faults: C# throws "Whitelist not found".
        let (state3, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings,
            None,
            "removeWhitelistFeeContract",
            3,
            &|b| {
                b.emit_push_int(1);
                b.emit_push("balanceOf".as_bytes());
                b.emit_push(&neo_hash.to_array());
            },
        );
        assert_eq!(
            state3,
            VmState::FAULT,
            "removing a missing whitelist must FAULT"
        );
    }

    /// setWhitelistFeeContract rejects a negative fixedFee before the committee
    /// check (C# ArgumentOutOfRangeException.ThrowIfNegative) and faults for an
    /// unknown method (C# "Method ... was not found").
    #[test]
    fn whitelist_fee_contract_e2e_validation_faults() {
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 0),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::NeoToken, &settings, 0),
        );
        let snapshot = Arc::new(cache);
        let signer = committee_address(&committee);
        let neo_hash = crate::NeoToken::script_hash();

        // Negative fixedFee -> FAULT.
        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings.clone(),
            None,
            "setWhitelistFeeContract",
            4,
            &|b| {
                b.emit_push_int(-1);
                b.emit_push_int(1);
                b.emit_push("balanceOf".as_bytes());
                b.emit_push(&neo_hash.to_array());
            },
        );
        assert_eq!(state, VmState::FAULT, "negative fixedFee must FAULT");

        // Unknown method name -> FAULT, nothing stored.
        let (state2, _) = call_policy(
            Arc::clone(&snapshot),
            signer,
            settings,
            None,
            "setWhitelistFeeContract",
            4,
            &|b| {
                b.emit_push_int(5);
                b.emit_push_int(0);
                b.emit_push("noexists".as_bytes());
                b.emit_push(&neo_hash.to_array());
            },
        );
        assert_eq!(state2, VmState::FAULT, "unknown method must FAULT");
        assert!(
            PolicyContract::new()
                .whitelist_fee_entries(&snapshot)
                .is_empty()
        );
    }

    /// recoverFund's verifiable prefix: the almost-full-committee gate (2-of-3
    /// here, max(max(1, n-(n-1)/2), n-2) = 2 for n = 3) plus the
    /// "Request not found." fault for an account that was never blocked.
    #[test]
    fn recover_fund_e2e_requires_request_and_committee() {
        const BLOCK_TIME_MS: u64 = 1_000_000;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        let snapshot = Arc::new(cache);
        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(BLOCK_TIME_MS);

        // Without the almost-full-committee witness -> FAULT.
        let stranger = UInt160::from_bytes(&[0x09; 20]).unwrap();
        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            stranger,
            settings.clone(),
            Some(Block::from_parts(header.clone(), vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array()); // token (arg 1, deeper)
                b.emit_push(&account.to_array()); // account (arg 0, top)
            },
        );
        assert_eq!(
            state,
            VmState::FAULT,
            "non-committee recoverFund must FAULT"
        );

        // With the witness but no blocked entry -> FAULT ("Request not found.").
        // For the 3-member sample committee the almost-full threshold equals the
        // regular committee threshold (both 2-of-3), so the same address signs.
        let (state2, _) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array());
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(
            state2,
            VmState::FAULT,
            "recoverFund without a request must FAULT"
        );
    }

    /// Seeds a GAS `AccountState` (`Struct[Balance]`) for `account`.
    fn seed_gas_balance(cache: &DataCache, account: &UInt160, balance: i64) {
        let state = StackItem::from_struct(vec![StackItem::from_int(balance)]);
        let mut key = vec![crate::NEP17_PREFIX_ACCOUNT];
        key.extend_from_slice(&account.to_bytes());
        cache.add(
            StorageKey::new(crate::GasToken::ID, key),
            StorageItem::from_bytes(
                BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap(),
            ),
        );
    }

    /// recoverFund happy path (C# `PolicyContract.RecoverFund`, lines 663-680):
    /// exactly one year after the blocked-account request, an almost-full
    /// committee signer sweeps the account's full GAS balance to Treasury
    /// through the VM — `balanceOf` then `transfer` issued from the native
    /// frame with `account` as the native calling script hash (authorizing the
    /// transfer via the `from == CallingScriptHash` bypass), Treasury's
    /// `onNEP17Payment` callback included — and emits `Transfer` followed by
    /// `RecoveredFund(account)`.
    #[test]
    fn recover_fund_e2e_sweeps_balance_to_treasury_and_notifies() {
        const REQUEST_TIME_MS: u64 = 1_000_000;
        const SWEPT: i64 = 123_456_789;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::GasToken, &settings, 100),
        );
        // Treasury must be a deployed contract so the GAS transfer's
        // onNEP17Payment callback runs (C# PostTransferAsync calls it whenever
        // the recipient is a contract).
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::Treasury, &settings, 100),
        );

        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let treasury = *crate::hashes::TREASURY_HASH;
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
        // The blocked-account entry carries the request's millisecond timestamp.
        cache.add(
            PolicyContract::blocked_account_key(&account),
            StorageItem::from_bytes(BigInt::from(REQUEST_TIME_MS).to_signed_bytes_le()),
        );
        seed_gas_balance(&cache, &account, SWEPT);
        let snapshot = Arc::new(cache);

        // Exactly one year elapsed: C# faults only when `elapsed < required`,
        // so the boundary block must pass.
        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

        let (state, engine) = call_policy_engine(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array()); // token (arg 1, deeper)
                b.emit_push(&account.to_array()); // account (arg 0, top)
            },
        );
        assert_eq!(
            state,
            VmState::HALT,
            "recoverFund sweep must HALT: {:?}",
            engine.fault_exception()
        );
        assert!(
            engine.result_stack().peek(0).unwrap().as_bool().unwrap(),
            "recoverFund returns true after a sweep"
        );

        // The full balance moved to Treasury; the account's entry was deleted
        // (an exact-balance NEP-17 transfer removes the from-record).
        assert_eq!(
            crate::read_nep17_balance(&snapshot, crate::GasToken::ID, &treasury).unwrap(),
            BigInt::from(SWEPT)
        );
        assert_eq!(
            crate::read_nep17_balance(&snapshot, crate::GasToken::ID, &account).unwrap(),
            BigInt::from(0)
        );
        // recoverFund does not unblock the account.
        assert!(
            snapshot
                .get(&PolicyContract::blocked_account_key(&account))
                .is_some()
        );

        // Notification order matches C#: the GAS Transfer (emitted inside the
        // nested transfer call) first, then Policy's RecoveredFund(account).
        let notifications = engine.notifications();
        assert_eq!(notifications.len(), 2, "expected Transfer + RecoveredFund");
        assert_eq!(notifications[0].script_hash, gas_hash);
        assert_eq!(notifications[0].event_name, "Transfer");
        assert_eq!(
            notifications[0].state[0].as_bytes().unwrap(),
            account.to_bytes()
        );
        assert_eq!(
            notifications[0].state[1].as_bytes().unwrap(),
            treasury.to_bytes()
        );
        assert_eq!(
            notifications[0].state[2].as_int().unwrap(),
            BigInt::from(SWEPT)
        );
        assert_eq!(notifications[1].script_hash, PolicyContract::script_hash());
        assert_eq!(notifications[1].event_name, "RecoveredFund");
        assert_eq!(
            notifications[1].state[0].as_bytes().unwrap(),
            account.to_bytes()
        );
    }

    /// recoverFund with a zero balance: C# `return false` — HALT, nothing
    /// moves, and neither Transfer nor RecoveredFund is emitted.
    #[test]
    fn recover_fund_e2e_zero_balance_returns_false() {
        const REQUEST_TIME_MS: u64 = 1_000_000;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::GasToken, &settings, 100),
        );

        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
        cache.add(
            PolicyContract::blocked_account_key(&account),
            StorageItem::from_bytes(BigInt::from(REQUEST_TIME_MS).to_signed_bytes_le()),
        );
        let snapshot = Arc::new(cache);

        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

        let (state, engine) = call_policy_engine(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array());
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(
            state,
            VmState::HALT,
            "zero-balance recoverFund must HALT: {:?}",
            engine.fault_exception()
        );
        assert!(
            !engine.result_stack().peek(0).unwrap().as_bool().unwrap(),
            "recoverFund returns false when there is nothing to sweep"
        );
        assert!(
            engine.notifications().is_empty(),
            "no Transfer/RecoveredFund for an empty sweep"
        );
        assert_eq!(
            crate::read_nep17_balance(
                &snapshot,
                crate::GasToken::ID,
                &crate::hashes::TREASURY_HASH
            )
            .unwrap(),
            BigInt::from(0)
        );
    }

    /// One millisecond short of the one-year window faults (C# "Request must
    /// be signed at least 1 year ago. Remaining time: …") and moves no funds.
    #[test]
    fn recover_fund_e2e_rejects_recent_request() {
        const REQUEST_TIME_MS: u64 = 1_000_000;
        const BALANCE: i64 = 777;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::GasToken, &settings, 100),
        );

        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
        cache.add(
            PolicyContract::blocked_account_key(&account),
            StorageItem::from_bytes(BigInt::from(REQUEST_TIME_MS).to_signed_bytes_le()),
        );
        seed_gas_balance(&cache, &account, BALANCE);
        let snapshot = Arc::new(cache);

        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND - 1);

        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&gas_hash.to_array());
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(state, VmState::FAULT, "a too-recent request must FAULT");
        assert_eq!(
            crate::read_nep17_balance(&snapshot, crate::GasToken::ID, &account).unwrap(),
            BigInt::from(BALANCE),
            "the balance must be untouched"
        );
    }

    /// A deployed token that does not declare the NEP-17 standard faults (C#
    /// "Contract {token} does not implement NEP-17 standard."). Treasury is a
    /// deployed non-NEP-17 contract, so it doubles as the token here.
    #[test]
    fn recover_fund_e2e_requires_nep17_standard() {
        const REQUEST_TIME_MS: u64 = 1_000_000;
        crate::install();
        let settings = faun_settings();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        deploy_native(
            &cache,
            &build_native_contract_state(&PolicyContract, &settings, 100),
        );
        deploy_native(
            &cache,
            &build_native_contract_state(&crate::Treasury, &settings, 100),
        );

        let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
        let treasury = *crate::hashes::TREASURY_HASH;
        cache.add(
            PolicyContract::blocked_account_key(&account),
            StorageItem::from_bytes(BigInt::from(REQUEST_TIME_MS).to_signed_bytes_le()),
        );
        let snapshot = Arc::new(cache);

        let mut header = BlockHeader::default();
        header.set_index(100);
        header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

        let (state, _) = call_policy(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings,
            Some(Block::from_parts(header, vec![])),
            "recoverFund",
            2,
            &|b| {
                b.emit_push(&treasury.to_array());
                b.emit_push(&account.to_array());
            },
        );
        assert_eq!(state, VmState::FAULT, "a non-NEP-17 token must FAULT");
    }
}
