//! The TemporaryStorage native contract (neo v3.11.2, `HF_Huyao`).
//!
//! Ported from `Neo.SmartContract.Native.TemporaryStorage`:
//!
//! - Records are stored under `Prefix_Record || contractId(int32 LE) || key`
//!   and carry an 8-byte big-endian `validTill` timestamp as a value prefix.
//! - Expiration metadata lives under
//!   `Prefix_ValidTill || validTill(uint64 BE) || contractId(int32 LE) || key`
//!   so `PostPersist` can sweep expired records in timestamp order.
//! - Records are owned by the *calling* contract (resolved through
//!   `ContractManagement.GetContractId`), not by this contract.
//! - Storage is charged dynamically through `CalculateStoragePrice`, on top of
//!   the fixed `1 << 15` CPU fee declared by each method.
//!
//! The contract itself has no `ActiveIn` hardfork: the manifest is composed at
//! `HF_Huyao` (`Activations`), which is when `ContractManagement` deploys it.

use super::method_macros::neo_native_methods;
use super::{ContractManagement, NativeContract, NativeMethod, PolicyContract};
use neo_primitives::UInt160;
use neo_primitives::find_options::FindOptions;
use neo_protocol::Hardfork;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreError as Error, CoreResult};
use neo_smart_contract::impl_native_contract;
use neo_smart_contract::iterators::storage_iterator::StorageIterator;
use neo_storage::StorageItem;
use neo_storage::StorageKey;
use neo_storage::persistence::{PersistenceDataCache, SeekDirection};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// `Prefix_Record`: key-value pair storage.
const PREFIX_RECORD: u8 = 0x01;
/// `Prefix_ValidTill`: expiration metadata storage.
const PREFIX_VALID_TILL: u8 = 0x02;
/// Maximum number of expired records removed by a single `PostPersist`.
const MAX_CLEANUP_BATCH_SIZE: usize = 10_000;

/// The TemporaryStorage native contract.
pub struct TemporaryStorage {
    id: i32,
    hash: UInt160,
    methods: &'static [NativeMethod],
}

impl TemporaryStorage {
    /// Contract ID. Appended after `Treasury` (-11), matching the C#
    /// `NativeContract` registration order.
    pub const ID: i32 = -12;
    /// Contract name (also feeds the contract hash).
    pub const NAME: &'static str = "TemporaryStorage";

    /// Creates a new TemporaryStorage contract.
    pub fn new() -> Self {
        Self {
            id: Self::ID,
            hash: neo_smart_contract::helper::Helper::get_contract_hash(
                &UInt160::zero(),
                0,
                Self::NAME,
            ),
            methods: crate::native::static_native_methods!(TemporaryStorage::native_methods()),
        }
    }

    /// Declares the TemporaryStorage method table in C# declaration order.
    fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            unsafe "put", fee = 1 << 15, flags = [STATES], params = [ByteArray, ByteArray, Integer], returns = Void, names = ["key", "value", "validTill"];
            safe "get", fee = 1 << 15, flags = [READ_STATES], params = [ByteArray], returns = ByteArray, nullable = true, names = ["key"];
            safe "get", fee = 1 << 15, flags = [READ_STATES], params = [Hash160, ByteArray], returns = ByteArray, nullable = true, names = ["hash", "key"];
            safe "getExpiration", fee = 1 << 15, flags = [READ_STATES], params = [ByteArray], returns = Integer, names = ["key"];
            safe "getExpiration", fee = 1 << 15, flags = [READ_STATES], params = [Hash160, ByteArray], returns = Integer, names = ["hash", "key"];
            unsafe "delete", fee = 1 << 15, flags = [STATES], params = [ByteArray], returns = Void, names = ["key"];
            safe "find", fee = 1 << 15, flags = [READ_STATES], params = [ByteArray, Integer], returns = InteropInterface, names = ["prefix", "options"];
            safe "find", fee = 1 << 15, flags = [READ_STATES], params = [Hash160, ByteArray, Integer], returns = InteropInterface, names = ["hash", "prefix", "options"];
            unsafe "renew", fee = 1 << 15, flags = [STATES], params = [ByteArray, Integer], returns = Void, names = ["key", "validTill"];
        ]
    }

    /// Dispatches a TemporaryStorage method invocation.
    ///
    /// Overloads (`get`, `getExpiration`, `find`) are resolved by argument
    /// count, matching C# overload resolution on the native registry.
    fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match (method, args.len()) {
            ("put", 3) => self.put(engine, args),
            ("get", 1) => self.get(engine, None, args),
            ("get", 2) => self.get(engine, Some(&args[0]), &args[1..]),
            ("getExpiration", 1) => self.get_expiration(engine, None, args),
            ("getExpiration", 2) => self.get_expiration(engine, Some(&args[0]), &args[1..]),
            ("delete", 1) => self.delete(engine, args),
            ("find", 2) => self.find(engine, None, &args[0], &args[1]),
            ("find", 3) => self.find(engine, Some(&args[0]), &args[1], &args[2]),
            ("renew", 2) => self.renew(engine, args),
            _ => Err(Error::invalid_operation(format!(
                "Method {method} not found in {}",
                Self::NAME
            ))),
        }
    }

    /// `put(key, value, validTill)`.
    fn put(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let key = &args[0];
        let value = &args[1];
        let valid_till = decode_u64(&args[2], "validTill")?;

        let timestamp = persisting_timestamp(engine)?;
        validate_valid_till(engine, valid_till, timestamp)?;

        let contract_id = calling_contract_id(engine)?;
        let record = record_suffix(contract_id, key);
        let context = engine.get_native_storage_context(&self.hash)?;

        let old = engine.get_storage_item(&context, &record);
        let (old_lifetime, old_valid_till) = match old.as_deref() {
            Some(old_value) => match traceable_valid_till(old_value, timestamp) {
                Some(stored) => (saturating_sub(valid_till.min(stored), timestamp), stored),
                None => (0, 0),
            },
            None => (0, 0),
        };
        let new_lifetime = saturating_sub(valid_till, timestamp.max(old_valid_till));

        let price = self.calculate_storage_price(
            engine,
            old.as_deref(),
            &record,
            value,
            old_lifetime,
            new_lifetime,
        );
        engine.charge_execution_fee(price)?;

        if let Some(old_value) = old.as_deref() {
            let stored_valid_till = read_valid_till(old_value);
            let stale = valid_till_suffix(stored_valid_till, &record[1..]);
            engine.delete_storage_item(&context, &stale)?;
        }

        put_record(engine, &self.hash, &record, value, valid_till)?;
        Ok(Vec::new())
    }

    /// `get(key)` / `get(hash, key)`.
    fn get(
        &self,
        engine: &mut ApplicationEngine,
        hash: Option<&Vec<u8>>,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let key = &args[0];
        let (value, _) = self.get_internal(engine, hash, key)?;
        Ok(value.unwrap_or_default())
    }

    /// `getExpiration(key)` / `getExpiration(hash, key)`.
    fn get_expiration(
        &self,
        engine: &mut ApplicationEngine,
        hash: Option<&Vec<u8>>,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let key = &args[0];
        let (_, valid_till) = self.get_internal(engine, hash, key)?;
        Ok(encode_u64(valid_till))
    }

    /// Shared implementation of the `get`/`getExpiration` overloads.
    fn get_internal(
        &self,
        engine: &mut ApplicationEngine,
        hash: Option<&Vec<u8>>,
        key: &[u8],
    ) -> CoreResult<(Option<Vec<u8>>, u64)> {
        let contract_id = match hash {
            Some(hash) => contract_id_of(engine, hash)?,
            None => calling_contract_id(engine)?,
        };
        let record = record_suffix(contract_id, key);
        let context = engine.get_native_storage_context(&self.hash)?;
        let Some(record_value) = engine.get_storage_item(&context, &record) else {
            return Ok((None, 0));
        };
        let timestamp = persisting_timestamp(engine)?;
        let Some(valid_till) = traceable_valid_till(&record_value, timestamp) else {
            return Ok((None, 0));
        };
        Ok((Some(record_value[8..].to_vec()), valid_till))
    }

    /// `delete(key)`.
    fn delete(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let key = &args[0];
        let contract_id = calling_contract_id(engine)?;
        let record = record_suffix(contract_id, key);
        let context = engine.get_native_storage_context(&self.hash)?;
        let Some(record_value) = engine.get_storage_item(&context, &record) else {
            return Ok(Vec::new());
        };

        let valid_till = read_valid_till(&record_value);
        engine.delete_storage_item(&context, &record)?;
        engine.delete_storage_item(&context, &valid_till_suffix(valid_till, &record[1..]))?;
        Ok(Vec::new())
    }

    /// `find(prefix, options)` / `find(hash, prefix, options)`.
    fn find(
        &self,
        engine: &mut ApplicationEngine,
        hash: Option<&Vec<u8>>,
        prefix: &[u8],
        options_bytes: &[u8],
    ) -> CoreResult<Vec<u8>> {
        let options_bits = decode_u64(options_bytes, "options")?;
        let options = FindOptions::from_bits(options_bits as u8).ok_or_else(|| {
            Error::invalid_operation(format!("Invalid FindOptions value: {options_bits}"))
        })?;
        options
            .validate()
            .map_err(|e| Error::invalid_operation(e.to_string()))?;

        let contract_id = match hash {
            Some(hash) => contract_id_of(engine, hash)?,
            None => calling_contract_id(engine)?,
        };
        let timestamp = persisting_timestamp(engine)?;

        let mut search = Vec::with_capacity(1 + 4 + prefix.len());
        search.push(PREFIX_RECORD);
        search.extend_from_slice(&contract_id.to_le_bytes());
        search.extend_from_slice(prefix);

        let direction = if options.contains(FindOptions::Backwards) {
            SeekDirection::Backward
        } else {
            SeekDirection::Forward
        };
        let entries: Vec<(StorageKey, StorageItem)> = engine
            .snapshot_cache()
            .find(Some(&StorageKey::new(self.id, search)), direction)
            .collect();

        let mut filtered = Vec::with_capacity(entries.len());
        for (key, item) in entries {
            let value = item.value_bytes();
            if traceable_valid_till(&value, timestamp).is_none() {
                continue;
            }
            // C# rewrites the key to the caller-visible form: drop the
            // `Prefix_Record || contractId` header so `prefix_length` strips the
            // user prefix only.
            let suffix = key.suffix().to_vec();
            if suffix.len() < 5 {
                continue;
            }
            filtered.push((
                StorageKey::new(self.id, suffix[5..].to_vec()),
                StorageItem::from_bytes(value[8..].to_vec()),
            ));
        }

        let iterator = StorageIterator::new(filtered, prefix.len(), options);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(Error::invalid_operation)?;
        Ok(iterator_id.to_le_bytes().to_vec())
    }

    /// `renew(key, validTill)`.
    fn renew(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let key = &args[0];
        let valid_till = decode_u64(&args[1], "validTill")?;

        let timestamp = persisting_timestamp(engine)?;
        validate_valid_till(engine, valid_till, timestamp)?;

        let contract_id = calling_contract_id(engine)?;
        let record = record_suffix(contract_id, key);
        let context = engine.get_native_storage_context(&self.hash)?;
        let Some(old_record) = engine.get_storage_item(&context, &record) else {
            return Err(Error::invalid_operation("old record not found".to_string()));
        };
        let Some(old_valid_till) = traceable_valid_till(&old_record, timestamp) else {
            return Err(Error::invalid_operation(
                "old record is expired".to_string(),
            ));
        };

        let value = old_record[8..].to_vec();
        let old_lifetime = saturating_sub(valid_till.min(old_valid_till), timestamp);
        let new_lifetime = saturating_sub(valid_till, old_valid_till);
        let price = self.calculate_storage_price(
            engine,
            Some(&old_record),
            &record,
            &value,
            old_lifetime,
            new_lifetime,
        );
        engine.charge_execution_fee(price)?;

        let stored_valid_till = read_valid_till(&old_record);
        engine.delete_storage_item(
            &context,
            &valid_till_suffix(stored_valid_till, &record[1..]),
        )?;
        put_record(engine, &self.hash, &record, &value, valid_till)?;
        Ok(Vec::new())
    }

    /// `CalculateStoragePrice`: the paid part for the existing lifetime plus the
    /// unpaid part for the new lifetime, each with a 10% floor to avoid DoS.
    fn calculate_storage_price(
        &self,
        engine: &ApplicationEngine,
        old_value: Option<&[u8]>,
        record_key: &[u8],
        value: &[u8],
        old_lifetime: u64,
        new_lifetime: u64,
    ) -> u64 {
        let (mut old_lifetime, mut new_lifetime) = (old_lifetime, new_lifetime);
        if old_lifetime == 0 && new_lifetime == 0 {
            // Pay at least the minimal fee in order not to DOS the contract.
            if old_value.is_none() {
                new_lifetime = 1;
            } else {
                old_lifetime = 1;
            }
        }

        let storage_price = i128::from(engine.storage_price());
        let max_ttl = u64::from(PolicyContract::MAX_TEMPORARY_STORAGE_MAX_TTL);
        let mut min_old: i128 = 0;
        let mut actual_old: i128 = 0;
        let mut min_new: i128 = 0;
        let mut actual_new: i128 = 0;

        // The "old" part uses permanent-storage rules for existing records; the
        // 8-byte TTL prefix is not charged.
        if old_lifetime > 0 {
            let chargable_value_size = old_value.map(|item| item.len().saturating_sub(8));
            let size = chargable_size(record_key, value.len(), chargable_value_size);
            let permanent = size as i128 * storage_price;
            min_old = permanent / 10;
            actual_old = permanent * i128::from(old_lifetime.min(max_ttl)) / i128::from(max_ttl);
        }

        // The "new" part always treats the record as non-traceable: the user has
        // not paid for it yet.
        if new_lifetime > 0 {
            let size = chargable_size(record_key, value.len(), None);
            let permanent = size as i128 * storage_price;
            min_new = permanent / 10;
            actual_new = permanent * i128::from(new_lifetime.min(max_ttl)) / i128::from(max_ttl);
        }

        let total = min_old.max(actual_old) + min_new.max(actual_new);
        u64::try_from(total).unwrap_or(u64::MAX)
    }
}

neo_storage::impl_default_via_new!(TemporaryStorage);

impl NativeContract for TemporaryStorage {
    impl_native_contract!(hash, Self::NAME, methods);

    fn id(&self) -> i32 {
        self.id
    }

    /// C# `NativeContract.ActiveIn => Activations.FirstOrDefault()`; for
    /// `TemporaryStorage` `Activations => [Hardfork.HF_Huyao]`, so the contract is
    /// inactive before Huyao and is *not* initialized at genesis
    /// (`IsInitializeBlock` requires `index == 0 && ActiveIn is null`).
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfHuyao)
    }

    /// Mirrors C# `Activations`, which is the set of hardforks the contract
    /// manifest is recomposed at.
    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfHuyao]
    }

    /// `PostPersist` removes records whose `validTill` has passed.
    ///
    /// Entries are visited in `validTill` order, so the sweep stops at the first
    /// record that is still alive; at most [`MAX_CLEANUP_BATCH_SIZE`] records are
    /// removed per block.
    fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let Some(block) = engine.persisting_block() else {
            return Ok(());
        };
        let timestamp = block.header.timestamp();
        let context = engine.get_native_storage_context(&self.hash)?;
        let expired = engine.find_storage_entries_with_prefix(&self.hash, &[PREFIX_VALID_TILL]);

        let mut count = 0usize;
        for (key, _) in expired {
            let suffix = key.suffix().to_vec();
            if suffix.len() < 9 {
                continue;
            }
            let valid_till = u64::from_be_bytes(
                suffix[1..9]
                    .try_into()
                    .expect("validTill prefix has 8 bytes"),
            );
            if valid_till >= timestamp {
                break;
            }

            engine.delete_storage_item(&context, &suffix)?;
            let mut record = Vec::with_capacity(1 + suffix.len() - 9);
            record.push(PREFIX_RECORD);
            record.extend_from_slice(&suffix[9..]);
            engine.delete_storage_item(&context, &record)?;

            count += 1;
            if count >= MAX_CLEANUP_BATCH_SIZE {
                break;
            }
        }
        Ok(())
    }
}

/// Builds the `Prefix_Record || contractId || key` storage suffix.
fn record_suffix(contract_id: i32, key: &[u8]) -> Vec<u8> {
    let mut suffix = Vec::with_capacity(1 + 4 + key.len());
    suffix.push(PREFIX_RECORD);
    suffix.extend_from_slice(&contract_id.to_le_bytes());
    suffix.extend_from_slice(key);
    suffix
}

/// Builds the `Prefix_ValidTill || validTill || contractId || key` suffix.
///
/// `record_suffix` is the matching `Prefix_Record` suffix; its first byte is
/// dropped so both keys share `contractId || key`.
fn valid_till_suffix(valid_till: u64, record_suffix: &[u8]) -> Vec<u8> {
    let mut suffix = Vec::with_capacity(1 + 8 + record_suffix.len());
    suffix.push(PREFIX_VALID_TILL);
    suffix.extend_from_slice(&valid_till.to_be_bytes());
    suffix.extend_from_slice(record_suffix);
    suffix
}

/// Writes the record and its expiration metadata.
fn put_record(
    engine: &mut ApplicationEngine,
    contract_hash: &UInt160,
    record: &[u8],
    value: &[u8],
    valid_till: u64,
) -> CoreResult<()> {
    let mut record_value = Vec::with_capacity(8 + value.len());
    record_value.extend_from_slice(&valid_till.to_be_bytes());
    record_value.extend_from_slice(value);

    let context = engine.get_native_storage_context(contract_hash)?;
    engine.put_storage_item(&context, record, &record_value)?;
    engine.put_storage_item(&context, &valid_till_suffix(valid_till, &record[1..]), &[])?;
    Ok(())
}

/// Reads the stored `validTill` (8 bytes big-endian value prefix).
fn read_valid_till(record_value: &[u8]) -> u64 {
    u64::from_be_bytes(
        record_value[..8]
            .try_into()
            .expect("temporary record starts with validTill"),
    )
}

/// Returns the record's `validTill` when it has not expired yet.
fn traceable_valid_till(record_value: &[u8], timestamp: u64) -> Option<u64> {
    if record_value.len() < 8 {
        return None;
    }
    let valid_till = read_valid_till(record_value);
    (valid_till >= timestamp).then_some(valid_till)
}

/// `ApplicationEngine.CalculateChargableSize` for a single record.
///
/// `chargable_value_size` is `None` when the record does not exist or must be
/// treated as non-traceable; otherwise it is the number of value bytes that were
/// already charged.
fn chargable_size(key: &[u8], value_len: usize, chargable_value_size: Option<usize>) -> usize {
    let Some(chargable_value_size) = chargable_value_size else {
        return key.len() + value_len;
    };
    if value_len == 0 {
        0
    } else if value_len <= chargable_value_size {
        value_len.saturating_sub(1) / 4 + 1
    } else if chargable_value_size == 0 {
        value_len
    } else {
        (chargable_value_size - 1) / 4 + 1 + value_len - chargable_value_size
    }
}

/// Saturating subtraction used by the lifetime arithmetic.
fn saturating_sub(a: u64, b: u64) -> u64 {
    a.saturating_sub(b)
}

/// Ensures `validTill` fits `[timestamp, timestamp + MaxTTL]`.
fn validate_valid_till(
    engine: &ApplicationEngine,
    valid_till: u64,
    timestamp: u64,
) -> CoreResult<()> {
    if valid_till < timestamp {
        return Err(Error::invalid_operation(format!(
            "validTill has already expired: {valid_till} vs {timestamp}"
        )));
    }
    let max_ttl =
        PolicyContract::get_temporary_storage_max_ttl_snapshot(engine.snapshot_cache().as_ref())
            .ok_or_else(|| {
                Error::invalid_operation("TemporaryStorageMaxTTL is not initialized".to_string())
            })?;
    let max_valid_till = timestamp
        .checked_add(max_ttl)
        .ok_or_else(|| Error::invalid_operation("validTill overflow".to_string()))?;
    if valid_till > max_valid_till {
        return Err(Error::invalid_operation(format!(
            "validTill exceeds max limit: {valid_till} vs {max_valid_till}"
        )));
    }
    Ok(())
}

/// Returns the timestamp of the block being persisted.
fn persisting_timestamp(engine: &ApplicationEngine) -> CoreResult<u64> {
    engine
        .persisting_block()
        .map(|block| block.header.timestamp())
        .ok_or_else(|| {
            Error::invalid_operation("TemporaryStorage requires a persisting block.".to_string())
        })
}

/// Resolves the ID of the calling contract.
fn calling_contract_id(engine: &ApplicationEngine) -> CoreResult<i32> {
    let hash = engine.calling_script_hash();
    contract_id_of_cache(engine.snapshot_cache().as_ref(), &hash)
}

/// Resolves the ID of the contract with the given 20-byte hash argument.
fn contract_id_of(engine: &ApplicationEngine, hash: &[u8]) -> CoreResult<i32> {
    let hash = UInt160::from_bytes(hash)
        .map_err(|_| Error::invalid_operation("Invalid contract hash".to_string()))?;
    contract_id_of_cache(engine.snapshot_cache().as_ref(), &hash)
}

/// `ContractManagement.GetContractId`: the ID of a deployed contract.
fn contract_id_of_cache(snapshot: &PersistenceDataCache, hash: &UInt160) -> CoreResult<i32> {
    ContractManagement::get_contract_id(snapshot, hash)?
        .ok_or_else(|| Error::invalid_operation(format!("calling contract not found: {hash}")))
}

/// Decodes a `ulong` argument.
fn decode_u64(payload: &[u8], name: &str) -> CoreResult<u64> {
    BigInt::from_signed_bytes_le(payload)
        .to_u64()
        .ok_or_else(|| Error::invalid_operation(format!("{name} is out of the ulong range")))
}

/// Encodes a `ulong` as a signed little-endian integer payload.
fn encode_u64(value: u64) -> Vec<u8> {
    BigInt::from(value).to_signed_bytes_le()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chargable_size_matches_csharp_rules() {
        // New record: key + value.
        assert_eq!(chargable_size(&[0x01, 0, 0, 0, 0, b'k'], 10, None), 16);
        // Shrinking: (value - 1) / 4 + 1.
        assert_eq!(chargable_size(&[1, 2], 4, Some(100)), 1);
        assert_eq!(chargable_size(&[1, 2], 5, Some(100)), 2);
        // Empty value.
        assert_eq!(chargable_size(&[1, 2], 0, Some(100)), 0);
        // Growing from an empty record.
        assert_eq!(chargable_size(&[1, 2], 7, Some(0)), 7);
        // Growing: (charged - 1) / 4 + 1 + growth.
        assert_eq!(chargable_size(&[1, 2], 13, Some(5)), 2 + 8);
    }

    #[test]
    fn record_and_valid_till_suffixes_share_contract_and_key() {
        let record = record_suffix(-12, b"abc");
        assert_eq!(&record[..5], &[PREFIX_RECORD, 0xf4, 0xff, 0xff, 0xff]);
        assert_eq!(&record[5..], b"abc");

        let valid_till = valid_till_suffix(0x0102_0304_0506_0708, &record[1..]);
        assert_eq!(valid_till[0], PREFIX_VALID_TILL);
        assert_eq!(
            &valid_till[1..9],
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
        assert_eq!(&valid_till[9..], &record[1..]);
    }

    #[test]
    fn expired_records_are_not_traceable() {
        let mut record = 1_000u64.to_be_bytes().to_vec();
        record.extend_from_slice(b"value");
        assert_eq!(traceable_valid_till(&record, 1_000), Some(1_000));
        assert_eq!(traceable_valid_till(&record, 1_001), None);
        assert_eq!(traceable_valid_till(&record, 999), Some(1_000));
    }
}
