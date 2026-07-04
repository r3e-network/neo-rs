//! Shared storage-setting helpers for native contracts.
//!
//! Promotes the i64 setting read/write helpers from `PolicyContract` so that
//! Notary and Oracle can reuse them, and extracts the hardfork-gated u32
//! setting reader shared by three Policy snapshot getters.

use neo_config::{Hardfork, ProtocolSettings};
use neo_error::{CoreError, CoreResult};
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

// ===== Promoted i64 setting helpers (from PolicyContract) =====

/// Reads an optional i64 setting from `snapshot` under `key`.
///
/// Returns `Ok(None)` when the key is absent, `Ok(Some(value))` when the
/// stored `BigInteger` fits in an `i64`, or an error when it does not.
///
/// Promoted from `PolicyContract::read_optional_i64_setting_key` so that
/// Notary (`read_max_not_valid_before_delta`) and Oracle (`read_price`) can
/// reuse the same logic.
pub(crate) fn read_optional_i64_setting_key(
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
                    "{setting} storage integer out of range"
                ))
            }),
        None => Ok(None),
    }
}

/// Reads a required i64 setting from `snapshot` under `key`.
///
/// Faults when the key is absent (matching the C# direct-index `snapshot[key]`
/// throw). Delegates to [`read_optional_i64_setting_key`].
pub(crate) fn read_required_i64_setting_key(
    snapshot: &DataCache,
    key: StorageKey,
    setting: &str,
) -> CoreResult<i64> {
    read_optional_i64_setting_key(snapshot, key, setting)?.ok_or_else(|| {
        CoreError::invalid_operation(format!("{setting} storage is missing"))
    })
}

/// Overwrites a required i64 setting under `key`.
///
/// Faults when the key has not been previously written (matching C#
/// `GetAndChange(...)` which throws on a missing key). The value is stored as
/// a little-endian `BigInteger`, byte-identical to the inlined code.
pub(crate) fn put_required_i64_setting_key(
    snapshot: &DataCache,
    key: StorageKey,
    setting: &str,
    value: i64,
) -> CoreResult<()> {
    if snapshot.get(&key).is_none() {
        return Err(CoreError::invalid_operation(format!(
            "{setting} storage is missing"
        )));
    }
    snapshot.update(
        key,
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
    );
    Ok(())
}

// ===== Hardfork-gated u32 setting reader =====

/// Reads a hardfork-gated u32 setting from `snapshot`.
///
/// Before `hardfork` is enabled (at the current ledger height) this returns
/// the `default` protocol setting. From `hardfork` onward it reads the stored
/// `BigInteger` under `key`, falling back to `default` when the key is absent
/// (the C# pre-genesis missing-key fallback).
///
/// Replaces 3 `get_max_*_snapshot` functions in `PolicyContract::storage`:
/// - `get_max_valid_until_block_increment_snapshot` (HfEchidna)
/// - `get_max_traceable_blocks_snapshot` (HfEchidna)
/// - `get_max_transactions_per_block_snapshot` (HfGorgon)
///
/// The "current block is missing" fallback mirrors the C# extension's
/// pre-genesis behaviour: when the ledger has no current block (e.g. during
/// genesis construction) the protocol default is returned.
pub(crate) fn read_hardfork_gated_u32_setting(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    default: u32,
    hardfork: Hardfork,
    key: StorageKey,
    label: &str,
) -> CoreResult<u32> {
    let height = match crate::LedgerContract::new().current_index(snapshot) {
        Ok(height) => height,
        Err(err) if err.to_string().contains("current block is missing") => return Ok(default),
        Err(err) => return Err(err),
    };
    if !settings.is_hardfork_enabled(hardfork, height) {
        return Ok(default);
    }
    let value = match read_optional_i64_setting_key(snapshot, key, label)? {
        Some(value) => value,
        None => return Ok(default),
    };
    u32::try_from(value)
        .map_err(|_| CoreError::invalid_operation(format!("{label} out of u32 range")))
}
