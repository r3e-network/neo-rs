//!
//! hardfork_ops.rs - On-chain hardfork activation and TemporaryStorage TTL
//! management for PolicyContract.
//!
//! Ported from C# `Neo.SmartContract.Native.PolicyContract` v3.11.2:
//!
//! - `GetHardforkActivationHeight` / `ActivateHardfork` (introduced at
//!   `HF_Huyao`), backed by storage prefix 25 keyed by the raw hardfork name.
//! - `GetTemporaryStorageMaxTTL` / `SetTemporaryStorageMaxTTL`, backed by
//!   storage prefix 24 and consumed by the `TemporaryStorage` native contract.
//! - `TryGetActivationHeight` / `IsHardforkEnabled`, the combined
//!   configuration + Policy-storage resolution used after `HF_Huyao`.
//!
//! C# keeps the last configuration-managed hardfork at `HF_Huyao`
//! (`ProtocolSettings.LastConfigManagedHardfork`); later hardforks are only
//! ever activated through `activateHardfork`.

use super::*;

impl PolicyContract {
    /// Reads the activation height of a hardfork from Policy storage.
    ///
    /// Returns `None` when the hardfork has no on-chain activation record.
    pub fn try_get_activation_height_from_storage<S>(
        snapshot: &S,
        hardfork: Hardfork,
    ) -> Option<u32>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = Self::hardfork_key(hardfork);
        snapshot
            .try_get(&key)
            .and_then(|item| BigInt::from_signed_bytes_le(&item.value_bytes()).to_u32())
    }

    /// Reads every activation height recorded in Policy storage.
    ///
    /// Used by the engine to resolve hardfork checks against on-chain
    /// activations without a per-check storage read (see
    /// `ApplicationEngine::is_hardfork_enabled_at`). Unknown names and
    /// out-of-range values are skipped rather than propagated, because this
    /// runs on read-only paths that must not fault.
    pub fn all_hardfork_heights<S>(snapshot: &S) -> Vec<(Hardfork, u32)>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let prefix = Self::hardfork_prefix();
        snapshot
            .find(Some(&prefix), neo_storage::SeekDirection::Forward)
            .filter_map(|(key, item)| {
                let suffix = key.suffix();
                if suffix.first().copied() != Some(Self::PREFIX_HARDFORK) {
                    return None;
                }
                let name = std::str::from_utf8(&suffix[1..]).ok()?;
                let hardfork = Hardfork::try_parse_exact(name)?;
                let height = BigInt::from_signed_bytes_le(&item.value_bytes()).to_u32()?;
                Some((hardfork, height))
            })
            .collect()
    }

    /// Resolves the activation height of a hardfork from configuration and the
    /// on-chain Policy storage.
    ///
    /// After `HF_Huyao` is enabled, hardforks up to and including it are read
    /// from Policy storage (whose values were copied from the configuration at
    /// the Huyao initialization block). Until then they come from
    /// `ProtocolSettings`. Later hardforks are always Policy-only.
    ///
    /// The `current_index - 1` check mirrors C#: Policy has ID -7, so its
    /// storage is only guaranteed to be initialized once Huyao is enabled
    /// *before* the block being evaluated.
    pub fn try_get_activation_height<S>(
        settings: &ProtocolSettings,
        snapshot: &S,
        hardfork: Hardfork,
        current_index: u32,
    ) -> Option<u32>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if !hardfork.is_config_managed() {
            return Self::try_get_activation_height_from_storage(snapshot, hardfork);
        }

        if current_index > 0
            && settings.is_hardfork_enabled(Hardfork::LAST_CONFIG_MANAGED, current_index - 1)
        {
            return Self::try_get_activation_height_from_storage(snapshot, hardfork);
        }

        settings.hardforks.get(&hardfork).copied()
    }

    /// Combined hardfork check: Policy storage for configuration-managed
    /// hardforks once Huyao is enabled, Policy-only for later hardforks, and
    /// `ProtocolSettings` before that.
    ///
    /// Matches C# `PolicyContract.IsHardforkEnabled(settings, snapshot, hf, index)`.
    pub fn is_hardfork_enabled<S>(
        settings: &ProtocolSettings,
        snapshot: &S,
        hardfork: Hardfork,
        index: u32,
    ) -> bool
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        match Self::try_get_activation_height(settings, snapshot, hardfork, index) {
            Some(height) => index >= height,
            None => false,
        }
    }

    /// Reads `TemporaryStorageMaxTTL` from a snapshot.
    pub fn get_temporary_storage_max_ttl_snapshot<S>(snapshot: &S) -> Option<u64>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = Self::temporary_storage_max_ttl_key();
        snapshot
            .try_get(&key)
            .and_then(|item| BigInt::from_signed_bytes_le(&item.value_bytes()).to_u64())
    }

    /// `getTemporaryStorageMaxTTL` - maximum TTL allowed by TemporaryStorage.
    pub(super) fn get_temporary_storage_max_ttl(
        &self,
        engine: &mut ApplicationEngine,
    ) -> Result<Vec<u8>> {
        let value = Self::get_temporary_storage_max_ttl_snapshot(engine.snapshot_cache().as_ref())
            .ok_or_else(|| {
                Error::native_contract("TemporaryStorageMaxTTL is not initialized".to_string())
            })?;
        Ok(Self::encode_u64(value))
    }

    /// `setTemporaryStorageMaxTTL` - committee-only update of the TTL cap.
    pub(super) fn set_temporary_storage_max_ttl(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "setTemporaryStorageMaxTTL requires value argument".to_string(),
            ));
        }

        let value = Self::parse_u64_argument(&args[0], "value")?;
        if value > Self::MAX_TEMPORARY_STORAGE_MAX_TTL {
            return Err(Error::invalid_operation(format!(
                "TemporaryStorageMaxTTL must be between [0, {}], got {value}",
                Self::MAX_TEMPORARY_STORAGE_MAX_TTL
            )));
        }

        Self::assert_committee(engine)?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::temporary_storage_max_ttl_key().suffix(),
            &Self::encode_u64(value),
        )?;
        Ok(Vec::new())
    }

    /// `getHardforkActivationHeight` - block height of an on-chain hardfork
    /// activation, or `null` for unknown/unscheduled hardforks.
    ///
    /// The ABI type is `Integer` (C# `BigInteger?`), so the unscheduled case
    /// returns an empty payload, which the engine renders as a null stack item.
    pub(super) fn get_hardfork_activation_height(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "getHardforkActivationHeight requires hardfork argument".to_string(),
            ));
        }

        let name = std::str::from_utf8(&args[0])
            .map_err(|_| Error::invalid_operation("Hardfork name is not valid UTF-8"))?;

        let Some(hardfork) = Hardfork::try_parse_exact(name) else {
            return Ok(Vec::new());
        };

        match Self::try_get_activation_height_from_storage(
            engine.snapshot_cache().as_ref(),
            hardfork,
        ) {
            Some(height) => Ok(Self::encode_u32(height)),
            None => Ok(Vec::new()),
        }
    }

    /// `activateHardfork` - committee-only scheduling of a post-Huyao hardfork.
    pub(super) fn activate_hardfork(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::native_contract(
                "activateHardfork requires hardfork and activationDelay arguments".to_string(),
            ));
        }

        let name = std::str::from_utf8(&args[0])
            .map_err(|_| Error::invalid_operation("Hardfork name is not valid UTF-8"))?
            .to_string();
        let activation_delay = Self::parse_u32_argument(&args[1], "activationDelay")?;

        if activation_delay < 1 {
            return Err(Error::invalid_operation(
                "Hardfork activation delay should be positive.".to_string(),
            ));
        }

        // Committee first: a random sender must not be able to halt the node
        // with a bogus name.
        Self::assert_committee(engine)?;

        let Some(hardfork) = Hardfork::try_parse_exact(&name) else {
            // C# v3.11.2 throws `UnknownHardforkException` here: after the
            // committee check, `Blockchain.Persist` rethrows it, so an outdated
            // node halts following the chain instead of silently diverging.
            return Err(Error::unknown_hardfork(name));
        };

        // Configuration-managed hardforks (through Huyao) cannot be activated
        // via Policy.
        if hardfork.is_config_managed() {
            return Err(Error::invalid_operation(format!(
                "Hardfork {name} must be activated via ProtocolSettings configuration, not Policy."
            )));
        }

        let persisting_index = engine
            .persisting_block()
            .map(|block| block.header.index())
            .ok_or_else(|| {
                Error::invalid_operation(
                    "Cannot activate hardfork without a persisting block.".to_string(),
                )
            })?;

        let activation_height =
            persisting_index
                .checked_add(activation_delay)
                .ok_or_else(|| {
                    Error::invalid_operation("Hardfork activation height overflow.".to_string())
                })?;

        if let Some(existing) =
            Self::try_get_activation_height_from_storage(engine.snapshot_cache().as_ref(), hardfork)
            && (existing <= persisting_index || existing == activation_height)
        {
            // Already active at or before this block, or a no-op reschedule to
            // the same height.
            return Err(Error::invalid_operation(format!(
                "Hardfork {name} is already scheduled at {existing}."
            )));
        }

        // Ensure the new activation height is aligned with the heights of the
        // other scheduled hardforks, and that continuity is preserved.
        let mut ancestor_scheduled = false;
        let ancestor_index = hardfork.to_byte().checked_sub(1);
        let scheduled =
            engine.find_storage_entries_with_prefix(&self.hash, &[Self::PREFIX_HARDFORK]);
        for (key, item) in scheduled {
            let suffix = key.suffix();
            if suffix.first().copied() != Some(Self::PREFIX_HARDFORK) || suffix.len() < 2 {
                continue;
            }
            let other_name = std::str::from_utf8(&suffix[1..]).map_err(|_| {
                Error::invalid_operation(
                    "An unknown hardfork is found in the Policy storage.".to_string(),
                )
            })?;
            let Some(other) = Hardfork::try_parse_exact(other_name) else {
                return Err(Error::invalid_operation(format!(
                    "An unknown hardfork {other_name} is found in the Policy storage."
                )));
            };
            let other_height = BigInt::from_signed_bytes_le(&item.value_bytes())
                .to_u32()
                .ok_or_else(|| {
                    Error::invalid_operation(format!(
                        "Hardfork {other_name} activation height is out of range."
                    ))
                })?;

            if (other < hardfork && other_height > activation_height)
                || (other > hardfork && other_height < activation_height)
            {
                return Err(Error::invalid_operation(format!(
                    "Hardfork {name} scheduled at {activation_height} conflicts with hardfork {} scheduled at {other_height}.",
                    other.raw_name()
                )));
            }

            if Some(other.to_byte()) == ancestor_index {
                ancestor_scheduled = true;
            }
        }

        if !ancestor_scheduled {
            return Err(Error::invalid_operation(
                "Hardfork-ancestor is not scheduled yet.".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::hardfork_key(hardfork).suffix(),
            &Self::encode_u32(activation_height),
        )?;

        engine
            .send_notification(
                self.hash,
                Self::HARDFORK_ACTIVATION_SCHEDULED_EVENT_NAME.to_string(),
                vec![
                    StackItem::ByteString(name.into_bytes()),
                    StackItem::from_int(i64::from(activation_height)),
                ],
            )
            .native_err()?;

        Ok(Vec::new())
    }
}
