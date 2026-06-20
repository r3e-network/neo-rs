mod recovery;
mod whitelist;

pub(super) use self::whitelist::WhitelistedContractView;

use super::{
    DEFAULT_ATTRIBUTE_FEE, DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE, FEE_FACTOR, MAX_EXEC_FEE_FACTOR,
    MAX_FEE_PER_BYTE, MAX_MAX_TRACEABLE_BLOCKS, MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
    MAX_MILLISECONDS_PER_BLOCK, MAX_STORAGE_PRICE, PREFIX_ATTRIBUTE_FEE, PREFIX_BLOCKED_ACCOUNT,
    PREFIX_EXEC_FEE_FACTOR, PREFIX_FEE_PER_BYTE, PREFIX_MAX_TRACEABLE_BLOCKS,
    PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT, PREFIX_MILLISECONDS_PER_BLOCK, PREFIX_STORAGE_PRICE,
    PolicyContract,
};
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_primitives::{TransactionAttributeType, UInt160};
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl PolicyContract {
    pub(super) fn setting_key(prefix: u8) -> StorageKey {
        crate::keys::prefixed_key(Self::ID, prefix, &[])
    }

    pub(crate) fn fee_per_byte_key() -> StorageKey {
        Self::setting_key(PREFIX_FEE_PER_BYTE)
    }

    pub(crate) fn storage_price_key() -> StorageKey {
        Self::setting_key(PREFIX_STORAGE_PRICE)
    }

    pub(crate) fn exec_fee_factor_key() -> StorageKey {
        Self::setting_key(PREFIX_EXEC_FEE_FACTOR)
    }

    pub(crate) fn milliseconds_per_block_key() -> StorageKey {
        Self::setting_key(PREFIX_MILLISECONDS_PER_BLOCK)
    }

    pub(crate) fn max_valid_until_block_increment_key() -> StorageKey {
        Self::setting_key(PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT)
    }

    pub(crate) fn max_traceable_blocks_key() -> StorageKey {
        Self::setting_key(PREFIX_MAX_TRACEABLE_BLOCKS)
    }

    pub(super) fn read_optional_i64_setting_key(
        snapshot: &DataCache,
        key: StorageKey,
        setting: &str,
    ) -> CoreResult<Option<i64>> {
        match snapshot.get(&key) {
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

    pub(super) fn read_required_i64_setting_key(
        snapshot: &DataCache,
        key: StorageKey,
        setting: &str,
    ) -> CoreResult<i64> {
        Self::read_optional_i64_setting_key(snapshot, key, setting)?.ok_or_else(|| {
            CoreError::invalid_operation(format!("PolicyContract {setting} storage is missing"))
        })
    }

    pub(super) fn put_required_i64_setting_key(
        snapshot: &DataCache,
        key: StorageKey,
        setting: &str,
        value: i64,
    ) -> CoreResult<()> {
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
        let value = match Self::read_optional_i64_setting_key(
            snapshot,
            Self::max_valid_until_block_increment_key(),
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
        let value = match Self::read_optional_i64_setting_key(
            snapshot,
            Self::max_traceable_blocks_key(),
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
        let raw = Self::read_required_i64_setting_key(
            snapshot,
            Self::exec_fee_factor_key(),
            "ExecFeeFactor",
        )?;
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
    pub(super) fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<i64> {
        Self::read_required_i64_setting_key(snapshot, Self::fee_per_byte_key(), "FeePerByte")
    }

    /// C# `GetStoragePrice` = `(uint)(BigInteger)snapshot[_storagePrice]`.
    pub(super) fn storage_price(&self, snapshot: &DataCache) -> CoreResult<i64> {
        Self::read_required_i64_setting_key(snapshot, Self::storage_price_key(), "StoragePrice")
    }

    /// C# `SetFeePerByte` range guard: the value must be in `[0, MAX_FEE_PER_BYTE]`.
    pub(super) fn validate_fee_per_byte(value: i64) -> CoreResult<()> {
        if !(0..=MAX_FEE_PER_BYTE).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "FeePerByte must be between [0, {MAX_FEE_PER_BYTE}], got {value}"
            )));
        }
        Ok(())
    }

    /// Writes the fee-per-byte to `Prefix_FeePerByte` as a `BigInteger`, mirroring
    /// C# `GetAndChange(_feePerByte).Set(value)` (overwrite-as-Changed semantics).
    pub(super) fn put_fee_per_byte(&self, snapshot: &DataCache, value: i64) -> CoreResult<()> {
        Self::put_required_i64_setting_key(snapshot, Self::fee_per_byte_key(), "FeePerByte", value)
    }

    /// C# `SetStoragePrice` range guard: the value must be in `[1, MAX_STORAGE_PRICE]`
    /// (C# rejects `value == 0 || value > MaxStoragePrice`).
    pub(super) fn validate_storage_price(value: i64) -> CoreResult<()> {
        if !(1..=MAX_STORAGE_PRICE).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "StoragePrice must be between [1, {MAX_STORAGE_PRICE}], got {value}"
            )));
        }
        Ok(())
    }

    /// Writes the storage price to `Prefix_StoragePrice` as a `BigInteger`
    /// (C# `GetAndChange(_storagePrice).Set(value)`).
    pub(super) fn put_storage_price(&self, snapshot: &DataCache, value: i64) -> CoreResult<()> {
        Self::put_required_i64_setting_key(
            snapshot,
            Self::storage_price_key(),
            "StoragePrice",
            value,
        )
    }

    /// Reads the raw stored exec fee factor (`Prefix_ExecFeeFactor`). The value is
    /// the on-disk `BigInteger`; callers apply the HF_Faun pico-GAS scaling.
    pub(super) fn exec_fee_factor_raw(&self, snapshot: &DataCache) -> CoreResult<i64> {
        Self::read_required_i64_setting_key(snapshot, Self::exec_fee_factor_key(), "ExecFeeFactor")
    }

    /// C# `SetExecFeeFactor` range guard. The upper bound is `MaxExecFeeFactor`
    /// before HF_Faun and `FeeFactor * MaxExecFeeFactor` from HF_Faun onward; the
    /// value must be at least 1 (the C# parameter is `ulong`, so a non-positive value
    /// is rejected exactly like the `value == 0` check plus the unsigned binding).
    pub(super) fn validate_exec_fee_factor(
        &self,
        engine: &ApplicationEngine,
        value: i64,
    ) -> CoreResult<()> {
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
    pub(super) fn put_exec_fee_factor(&self, snapshot: &DataCache, value: i64) -> CoreResult<()> {
        Self::put_required_i64_setting_key(
            snapshot,
            Self::exec_fee_factor_key(),
            "ExecFeeFactor",
            value,
        )
    }

    /// C# attribute-type guard shared by get/setAttributeFee: the byte must be a
    /// defined `TransactionAttributeType`, and `NotaryAssisted` is only accepted when
    /// `allow_notary_assisted` (i.e. from HF_Echidna). Mirrors
    /// `!Enum.IsDefined(...) || (!allowNotaryAssisted && type == NotaryAssisted)`.
    pub(super) fn validate_attribute_type(
        attribute_type: u8,
        allow_notary_assisted: bool,
    ) -> CoreResult<()> {
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
    pub(crate) fn attribute_fee_key(attribute_type: u8) -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_ATTRIBUTE_FEE, &[attribute_type])
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
    pub(super) fn put_attribute_fee(&self, snapshot: &DataCache, attribute_type: u8, value: i64) {
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
                Self::milliseconds_per_block_key(),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    milliseconds_per_block,
                ))),
            );
            snapshot.add(
                Self::max_valid_until_block_increment_key(),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    max_valid_until_block_increment,
                ))),
            );
            snapshot.add(
                Self::max_traceable_blocks_key(),
                StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                    max_traceable_blocks,
                ))),
            );
        }
        if hardfork == Hardfork::HfFaun {
            // C# `GetAndChange(_execFeeFactor) ?? throw`: the factor must exist.
            let snapshot = engine.snapshot_cache();
            let factor_key = Self::exec_fee_factor_key();
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
        crate::keys::prefixed_hash160_key(Self::ID, PREFIX_BLOCKED_ACCOUNT, account)
    }

    /// The blocked-account prefix key `(PolicyContract.ID, [Prefix_BlockedAccount])`.
    pub(super) fn blocked_account_prefix_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_BLOCKED_ACCOUNT, &[])
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
    pub(super) fn blocked_account_entries(
        &self,
        snapshot: &DataCache,
    ) -> Vec<(StorageKey, StorageItem)> {
        let prefix_key = Self::blocked_account_prefix_key();
        snapshot
            .find(Some(&prefix_key), SeekDirection::Forward)
            .collect()
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
        if crate::is_standard_native_contract_hash(account) {
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

    /// C# `SetMillisecondsPerBlock` range guard: `[1, MaxMillisecondsPerBlock]`.
    pub(super) fn validate_milliseconds_per_block(value: i64) -> CoreResult<()> {
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
    pub(super) fn read_milliseconds_per_block(
        &self,
        engine: &ApplicationEngine,
    ) -> CoreResult<i64> {
        let snapshot = engine.snapshot_cache();
        Self::read_required_i64_setting_key(
            &snapshot,
            Self::milliseconds_per_block_key(),
            "MillisecondsPerBlock",
        )
    }

    /// Writes the milliseconds-per-block to `Prefix_MillisecondsPerBlock`
    /// (C# `GetAndChange(_millisecondsPerBlock).Set(value)`).
    pub(super) fn put_milliseconds_per_block(
        &self,
        snapshot: &DataCache,
        value: i64,
    ) -> CoreResult<()> {
        Self::put_required_i64_setting_key(
            snapshot,
            Self::milliseconds_per_block_key(),
            "MillisecondsPerBlock",
            value,
        )
    }

    /// C# `SetMaxValidUntilBlockIncrement` range guard: `[1, 86400]`.
    pub(super) fn validate_max_valid_until_block_increment(value: i64) -> CoreResult<()> {
        if !(1..=MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT).contains(&value) {
            return Err(CoreError::invalid_operation(format!(
                "MaxValidUntilBlockIncrement must be between [1, {MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT}], got {value}"
            )));
        }
        Ok(())
    }

    /// C# `SetMaxTraceableBlocks` range guard: `[1, 2102400]`.
    pub(super) fn validate_max_traceable_blocks(value: i64) -> CoreResult<()> {
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
        Self::read_required_i64_setting_key(
            &snapshot,
            Self::max_valid_until_block_increment_key(),
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
    pub(super) fn put_max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        value: i64,
    ) -> CoreResult<()> {
        Self::put_required_i64_setting_key(
            snapshot,
            Self::max_valid_until_block_increment_key(),
            "MaxValidUntilBlockIncrement",
            value,
        )
    }

    /// Writes `Prefix_MaxTraceableBlocks` (C# `GetAndChange(_maxTraceableBlocks).Set(value)`).
    pub(super) fn put_max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        value: i64,
    ) -> CoreResult<()> {
        Self::put_required_i64_setting_key(
            snapshot,
            Self::max_traceable_blocks_key(),
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
        let value = Self::read_required_i64_setting_key(
            &snapshot,
            Self::max_traceable_blocks_key(),
            "MaxTraceableBlocks",
        )?;
        u32::try_from(value)
            .map_err(|_| CoreError::invalid_operation("MaxTraceableBlocks out of u32 range"))
    }
}
